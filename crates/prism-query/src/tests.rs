use prism_coordination::{
    Artifact, ArtifactProposeInput, CoordinationPolicy, CoordinationSnapshot,
    CoordinationSnapshotV2, CoordinationSpecRef, CoordinationStore, CoordinationTask,
    CoordinationTaskSpecRef, HandoffInput, LeaseHolder, LeaseState, Plan, PlanCreateInput,
    PlanScheduling, RuntimeDescriptor, RuntimeDiscoveryMode, TaskCompletionContext,
    TaskCreateInput, TaskExecutorCaller, TaskGitExecution, TaskUpdateInput, WorkClaim,
};
use prism_history::HistoryStore;
use prism_ir::{
    new_prefixed_id, sortable_token_timestamp, AgentId, AnchorRef, ChangeTrigger,
    CoordinationTaskId, Edge, EdgeKind, EffectiveTaskStatus, EventActor, EventId, EventMeta,
    ExecutorClass, FileId, Language, Node, NodeId, NodeKind, ObservedChangeSet, ObservedNode,
    PlanId, PlanKind, PlanNodeKind, PlanScope, PlanStatus, PrincipalId, PrismRuntimeMode,
    SessionId, Span, TaskId, WorkspaceRevision,
};
use prism_memory::{
    OutcomeEvent, OutcomeEvidence, OutcomeKind, OutcomeMemory, OutcomeRecallQuery, OutcomeResult,
};
use prism_projections::{
    ConceptDecodeLens, ConceptPacket, ConceptProvenance, ConceptRelation, ConceptRelationKind,
    ConceptScope, ContractCompatibility, ContractGuarantee, ContractKind, ContractPacket,
    ContractScope, ContractStatus, ContractTarget, ProjectionIndex,
};
use prism_store::{CoordinationPersistContext, Graph};
use serde_json::json;

use super::{
    CoordinationTransactionError, CoordinationTransactionInput, CoordinationTransactionMutation,
    CoordinationTransactionPlanRef, CoordinationTransactionRejectionCategory,
    CoordinationTransactionValidationStage, NativeSpecPlanCreateInput, NativeSpecTaskCreateInput,
    Prism,
};

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
fn coordination_snapshot_preserves_task_lease_fields() {
    let task_id = CoordinationTaskId::new("coord-task:lease");
    let plan_id = PlanId::new("plan:lease");
    let prism = Prism::with_history_outcomes_coordination_and_projections(
        Graph::new(),
        HistoryStore::new(),
        OutcomeMemory::new(),
        CoordinationSnapshot {
            plans: vec![Plan {
                id: plan_id.clone(),
                goal: "ship".into(),
                title: "ship".into(),
                status: PlanStatus::Active,
                policy: CoordinationPolicy::default(),
                scope: PlanScope::Repo,
                kind: PlanKind::TaskExecution,
                revision: 1,
                scheduling: PlanScheduling::default(),
                tags: Vec::new(),
                created_from: None,
                spec_refs: Vec::new(),
                metadata: serde_json::Value::Null,
            }],
            tasks: vec![CoordinationTask {
                id: task_id.clone(),
                plan: plan_id,
                kind: PlanNodeKind::Edit,
                title: "Keep lease state".into(),
                summary: None,
                status: prism_ir::CoordinationTaskStatus::InProgress,
                published_task_status: None,
                assignee: Some(AgentId::new("agent:lease")),
                pending_handoff_to: None,
                session: Some(SessionId::new("session:lease")),
                lease_holder: Some(LeaseHolder {
                    principal: None,
                    session_id: Some(SessionId::new("session:lease")),
                    worktree_id: Some("worktree:lease".into()),
                    agent_id: Some(AgentId::new("agent:lease")),
                }),
                lease_started_at: Some(10),
                lease_refreshed_at: Some(11),
                lease_stale_at: Some(12),
                lease_expires_at: Some(13),
                worktree_id: Some("worktree:lease".into()),
                branch_ref: Some("refs/heads/task/lease".into()),
                anchors: Vec::new(),
                bindings: prism_ir::PlanBinding::default(),
                depends_on: Vec::new(),
                coordination_depends_on: Vec::new(),
                integrated_depends_on: Vec::new(),
                acceptance: Vec::new(),
                validation_refs: Vec::new(),
                is_abstract: false,
                base_revision: WorkspaceRevision::default(),
                priority: None,
                tags: Vec::new(),
                spec_refs: Vec::new(),
                metadata: serde_json::Value::Null,
                git_execution: TaskGitExecution::default(),
            }],
            claims: Vec::new(),
            artifacts: Vec::new(),
            reviews: Vec::new(),
            events: Vec::new(),
            next_plan: 1,
            next_task: 1,
            next_claim: 0,
            next_artifact: 0,
            next_review: 0,
        },
        ProjectionIndex::default(),
    );

    let task = prism
        .coordination_snapshot()
        .tasks
        .into_iter()
        .find(|task| task.id == task_id)
        .expect("leased task should survive snapshot rebuild");
    assert_eq!(task.assignee, Some(AgentId::new("agent:lease")));
    assert_eq!(task.session, Some(SessionId::new("session:lease")));
    assert_eq!(task.lease_started_at, Some(10));
    assert_eq!(task.lease_refreshed_at, Some(11));
    assert_eq!(task.lease_stale_at, Some(12));
    assert_eq!(task.lease_expires_at, Some(13));
    assert_eq!(
        task.lease_holder,
        Some(LeaseHolder {
            principal: None,
            session_id: Some(SessionId::new("session:lease")),
            worktree_id: Some("worktree:lease".into()),
            agent_id: Some(AgentId::new("agent:lease")),
        })
    );
}

#[test]
fn coordination_snapshot_v2_projects_legacy_snapshot_into_canonical_records() {
    let task_id = CoordinationTaskId::new("coord-task:lease");
    let plan_id = PlanId::new("plan:lease");
    let prism = Prism::with_history_outcomes_coordination_and_projections(
        Graph::new(),
        HistoryStore::new(),
        OutcomeMemory::new(),
        CoordinationSnapshot {
            plans: vec![Plan {
                id: plan_id.clone(),
                goal: "ship".into(),
                title: "ship".into(),
                status: PlanStatus::Active,
                policy: CoordinationPolicy::default(),
                scope: PlanScope::Repo,
                kind: PlanKind::TaskExecution,
                revision: 1,
                scheduling: PlanScheduling::default(),
                tags: Vec::new(),
                created_from: None,
                spec_refs: Vec::new(),
                metadata: serde_json::json!({"legacy": true}),
            }],
            tasks: vec![CoordinationTask {
                id: task_id.clone(),
                plan: plan_id.clone(),
                kind: PlanNodeKind::Edit,
                title: "Keep lease state".into(),
                summary: None,
                status: prism_ir::CoordinationTaskStatus::Ready,
                published_task_status: None,
                assignee: None,
                pending_handoff_to: Some(AgentId::new("agent:handoff")),
                session: None,
                lease_holder: None,
                lease_started_at: None,
                lease_refreshed_at: None,
                lease_stale_at: None,
                lease_expires_at: None,
                worktree_id: None,
                branch_ref: None,
                anchors: Vec::new(),
                bindings: prism_ir::PlanBinding::default(),
                depends_on: vec![CoordinationTaskId::new("coord-task:dep")],
                coordination_depends_on: Vec::new(),
                integrated_depends_on: Vec::new(),
                acceptance: Vec::new(),
                validation_refs: Vec::new(),
                is_abstract: false,
                base_revision: WorkspaceRevision::default(),
                priority: None,
                tags: Vec::new(),
                spec_refs: Vec::new(),
                metadata: serde_json::json!({"estimatedMinutes": 12}),
                git_execution: TaskGitExecution::default(),
            }],
            claims: Vec::new(),
            artifacts: Vec::new(),
            reviews: Vec::new(),
            events: Vec::new(),
            next_plan: 1,
            next_task: 1,
            next_claim: 0,
            next_artifact: 0,
            next_review: 0,
        },
        ProjectionIndex::default(),
    );

    let snapshot_v2: CoordinationSnapshotV2 = prism.coordination_snapshot_v2();
    assert_eq!(snapshot_v2.schema_version, 2);
    assert_eq!(snapshot_v2.plans.len(), 1);
    assert_eq!(snapshot_v2.tasks.len(), 1);
    assert_eq!(snapshot_v2.tasks[0].parent_plan_id, plan_id);
    assert_eq!(snapshot_v2.tasks[0].estimated_minutes, 12);
    assert_eq!(
        snapshot_v2.tasks[0].pending_handoff_to,
        Some(AgentId::new("agent:handoff"))
    );
    assert_eq!(snapshot_v2.dependencies.len(), 1);
    assert_eq!(
        snapshot_v2.dependencies[0].source.id,
        TaskId::new(task_id.0.clone()).0
    );
}

#[test]
fn plan_activity_falls_back_to_ids_and_embedded_timestamps_when_events_are_compacted() {
    let plan_id = PlanId::new(new_prefixed_id("plan"));
    let task_id = CoordinationTaskId::new(new_prefixed_id("coord-task"));
    let expected_created_at =
        sortable_token_timestamp(plan_id.0.as_str()).expect("plan id should encode a timestamp");
    let expected_last_updated_at = expected_created_at + 50;
    let prism = Prism::with_history_outcomes_coordination_and_projections(
        Graph::new(),
        HistoryStore::new(),
        OutcomeMemory::new(),
        CoordinationSnapshot {
            plans: vec![Plan {
                id: plan_id.clone(),
                goal: "Ship fallback".into(),
                title: "Ship fallback".into(),
                status: PlanStatus::Active,
                policy: CoordinationPolicy::default(),
                scope: PlanScope::Repo,
                kind: PlanKind::TaskExecution,
                revision: 1,
                scheduling: PlanScheduling::default(),
                tags: Vec::new(),
                created_from: None,
                spec_refs: Vec::new(),
                metadata: serde_json::Value::Null,
            }],
            tasks: vec![CoordinationTask {
                id: task_id.clone(),
                plan: plan_id.clone(),
                kind: PlanNodeKind::Edit,
                title: "Fallback task".into(),
                summary: None,
                status: prism_ir::CoordinationTaskStatus::InProgress,
                published_task_status: None,
                assignee: Some(AgentId::new("agent:fallback")),
                pending_handoff_to: None,
                session: Some(SessionId::new("session:fallback")),
                lease_holder: None,
                lease_started_at: Some(expected_created_at + 10),
                lease_refreshed_at: Some(expected_last_updated_at),
                lease_stale_at: Some(expected_last_updated_at + 30),
                lease_expires_at: Some(expected_last_updated_at + 60),
                worktree_id: None,
                branch_ref: None,
                anchors: Vec::new(),
                bindings: prism_ir::PlanBinding::default(),
                depends_on: Vec::new(),
                coordination_depends_on: Vec::new(),
                integrated_depends_on: Vec::new(),
                acceptance: Vec::new(),
                validation_refs: Vec::new(),
                is_abstract: false,
                base_revision: WorkspaceRevision::default(),
                priority: None,
                tags: Vec::new(),
                spec_refs: Vec::new(),
                metadata: serde_json::Value::Null,
                git_execution: TaskGitExecution::default(),
            }],
            claims: Vec::new(),
            artifacts: Vec::new(),
            reviews: Vec::new(),
            events: Vec::new(),
            next_plan: 1,
            next_task: 1,
            next_claim: 0,
            next_artifact: 0,
            next_review: 0,
        },
        ProjectionIndex::default(),
    );

    let activity = prism
        .plan_activity(&plan_id)
        .expect("active plan should surface backfilled activity");

    assert_eq!(activity.created_at, Some(expected_created_at));
    assert_eq!(activity.last_updated_at, Some(expected_last_updated_at));
    assert_eq!(activity.last_event_kind, None);
    assert_eq!(activity.last_event_summary, None);
    assert_eq!(activity.last_event_task_id, Some(task_id));
}

#[test]
fn effective_task_lease_state_joins_runtime_descriptors() {
    let plan_id = PlanId::new("plan:lease-runtime");
    let task_id = CoordinationTaskId::new("coord-task:lease-runtime");
    let prism = Prism::with_history_outcomes_coordination_and_projections(
        Graph::new(),
        HistoryStore::new(),
        OutcomeMemory::new(),
        CoordinationSnapshot {
            plans: vec![Plan {
                id: plan_id.clone(),
                goal: "ship".into(),
                title: "ship".into(),
                status: PlanStatus::Active,
                policy: CoordinationPolicy::default(),
                scope: PlanScope::Repo,
                kind: PlanKind::TaskExecution,
                revision: 1,
                scheduling: PlanScheduling::default(),
                tags: Vec::new(),
                created_from: None,
                spec_refs: Vec::new(),
                metadata: serde_json::Value::Null,
            }],
            tasks: vec![CoordinationTask {
                id: task_id.clone(),
                plan: plan_id,
                kind: PlanNodeKind::Edit,
                title: "Join runtime lease state".into(),
                summary: None,
                status: prism_ir::CoordinationTaskStatus::InProgress,
                published_task_status: None,
                assignee: None,
                pending_handoff_to: None,
                session: Some(SessionId::new("session:lease-runtime")),
                lease_holder: Some(LeaseHolder {
                    principal: None,
                    session_id: Some(SessionId::new("session:lease-runtime")),
                    worktree_id: Some("worktree:lease-runtime".into()),
                    agent_id: None,
                }),
                lease_started_at: Some(10),
                lease_refreshed_at: Some(10),
                lease_stale_at: Some(40),
                lease_expires_at: Some(70),
                worktree_id: Some("worktree:lease-runtime".into()),
                branch_ref: None,
                anchors: Vec::new(),
                bindings: prism_ir::PlanBinding::default(),
                depends_on: Vec::new(),
                coordination_depends_on: Vec::new(),
                integrated_depends_on: Vec::new(),
                acceptance: Vec::new(),
                validation_refs: Vec::new(),
                is_abstract: false,
                base_revision: WorkspaceRevision::default(),
                priority: None,
                tags: Vec::new(),
                spec_refs: Vec::new(),
                metadata: serde_json::Value::Null,
                git_execution: TaskGitExecution::default(),
            }],
            claims: Vec::new(),
            artifacts: Vec::new(),
            reviews: Vec::new(),
            events: Vec::new(),
            next_plan: 1,
            next_task: 1,
            next_claim: 0,
            next_artifact: 0,
            next_review: 0,
        },
        ProjectionIndex::default(),
    );

    let task = prism
        .coordination_task(&task_id)
        .expect("task should be present for lease join");
    assert_eq!(
        prism.effective_task_lease_state(&task, 50),
        LeaseState::Stale
    );

    prism.replace_runtime_descriptors(vec![RuntimeDescriptor {
        runtime_id: "runtime:lease-runtime".into(),
        repo_id: "repo:test".into(),
        worktree_id: "worktree:lease-runtime".into(),
        principal_id: "principal:test".into(),
        instance_started_at: 5,
        last_seen_at: 55,
        branch_ref: None,
        checked_out_commit: None,
        capabilities: Vec::new(),
        discovery_mode: RuntimeDiscoveryMode::None,
        peer_endpoint: None,
        public_endpoint: None,
        peer_transport_identity: None,
        blob_snapshot_head: None,
        export_policy: None,
    }]);

    assert_eq!(
        prism.effective_task_lease_state(&task, 50),
        LeaseState::Active
    );
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
fn authoritative_only_task_publish_intent_does_not_auto_complete_plan() {
    let store = CoordinationStore::new();
    let (plan_id, _) = store
        .create_plan(
            EventMeta {
                id: EventId::new("coord:plan:create"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            PlanCreateInput {
                title: "Ship it".into(),
                goal: "Ship it".into(),
                status: Some(PlanStatus::Active),
                policy: None,
                spec_refs: Vec::new(),
            },
        )
        .unwrap();
    let (task_id, _) = store
        .create_task(
            EventMeta {
                id: EventId::new("coord:task:create"),
                ts: 2,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Finish publish".into(),
                status: Some(prism_ir::CoordinationTaskStatus::InProgress),
                assignee: None,
                session: Some(SessionId::new("session:test")),
                worktree_id: None,
                branch_ref: None,
                anchors: Vec::new(),
                depends_on: Vec::new(),
                coordination_depends_on: Vec::new(),
                integrated_depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: WorkspaceRevision::default(),
                spec_refs: Vec::new(),
            },
        )
        .unwrap();

    let prism = Prism::with_history_outcomes_coordination_and_projections(
        Graph::new(),
        HistoryStore::default(),
        OutcomeMemory::default(),
        store.snapshot(),
        ProjectionIndex::default(),
    );

    let task = prism
        .update_native_task_authoritative_only(
            EventMeta {
                id: EventId::new("coord:task:publish-intent"),
                ts: 3,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            TaskUpdateInput {
                task_id: task_id.clone(),
                kind: None,
                status: None,
                published_task_status: Some(Some(prism_ir::CoordinationTaskStatus::Completed)),
                git_execution: Some(TaskGitExecution {
                    status: prism_ir::GitExecutionStatus::PublishPending,
                    pending_task_status: Some(prism_ir::CoordinationTaskStatus::Completed),
                    ..TaskGitExecution::default()
                }),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                title: None,
                summary: None,
                anchors: None,
                bindings: None,
                depends_on: None,
                coordination_depends_on: None,
                integrated_depends_on: None,
                acceptance: None,
                validation_refs: None,
                is_abstract: None,
                base_revision: Some(WorkspaceRevision::default()),
                priority: None,
                tags: None,
                completion_context: Some(TaskCompletionContext::default()),
                spec_refs: None,
            },
            WorkspaceRevision::default(),
            3,
        )
        .unwrap();

    assert_eq!(task.status, EffectiveTaskStatus::Active);
    assert_eq!(
        task.task
            .metadata
            .get("legacy_published_task_status")
            .and_then(serde_json::Value::as_str),
        Some("completed")
    );
    assert_eq!(task.task.parent_plan_id, plan_id);
    assert_eq!(
        task.task.git_execution.pending_task_status,
        Some(prism_ir::CoordinationTaskStatus::Completed)
    );
    assert_eq!(
        prism.coordination_plan(&plan_id).unwrap().status,
        PlanStatus::Active
    );
}

#[test]
fn authoritative_only_final_publication_auto_completes_plan() {
    let store = CoordinationStore::new();
    let (plan_id, _) = store
        .create_plan(
            EventMeta {
                id: EventId::new("coord:plan:create"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            PlanCreateInput {
                title: "Ship it".into(),
                goal: "Ship it".into(),
                status: Some(PlanStatus::Active),
                policy: None,
                spec_refs: Vec::new(),
            },
        )
        .unwrap();
    let (task_id, _) = store
        .create_task(
            EventMeta {
                id: EventId::new("coord:task:create"),
                ts: 2,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Finish publish".into(),
                status: Some(prism_ir::CoordinationTaskStatus::InProgress),
                assignee: None,
                session: Some(SessionId::new("session:test")),
                worktree_id: None,
                branch_ref: None,
                anchors: Vec::new(),
                depends_on: Vec::new(),
                coordination_depends_on: Vec::new(),
                integrated_depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: WorkspaceRevision::default(),
                spec_refs: Vec::new(),
            },
        )
        .unwrap();

    let prism = Prism::with_history_outcomes_coordination_and_projections(
        Graph::new(),
        HistoryStore::default(),
        OutcomeMemory::default(),
        store.snapshot(),
        ProjectionIndex::default(),
    );

    let task = prism
        .update_native_task_authoritative_only(
            EventMeta {
                id: EventId::new("coord:task:published"),
                ts: 3,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            TaskUpdateInput {
                task_id: task_id.clone(),
                kind: None,
                status: Some(prism_ir::CoordinationTaskStatus::Completed),
                published_task_status: Some(None),
                git_execution: Some(TaskGitExecution {
                    status: prism_ir::GitExecutionStatus::CoordinationPublished,
                    integration_mode: prism_ir::GitIntegrationMode::External,
                    integration_status: prism_ir::GitIntegrationStatus::PublishedToBranch,
                    pending_task_status: None,
                    ..TaskGitExecution::default()
                }),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                title: None,
                summary: None,
                anchors: None,
                bindings: None,
                depends_on: None,
                coordination_depends_on: None,
                integrated_depends_on: None,
                acceptance: None,
                validation_refs: None,
                is_abstract: None,
                base_revision: Some(WorkspaceRevision::default()),
                priority: None,
                tags: None,
                completion_context: Some(TaskCompletionContext::default()),
                spec_refs: None,
            },
            WorkspaceRevision::default(),
            3,
        )
        .unwrap();

    assert_eq!(task.status, EffectiveTaskStatus::Completed);
    assert_eq!(
        task.task.git_execution.status,
        prism_ir::GitExecutionStatus::CoordinationPublished
    );
    assert_eq!(
        prism.coordination_plan(&plan_id).unwrap().status,
        PlanStatus::Completed
    );
}

#[test]
fn authoritative_only_final_publication_bypasses_expired_same_holder_lease() {
    let store = CoordinationStore::new();
    let (plan_id, _) = store
        .create_plan(
            EventMeta {
                id: EventId::new("coord:plan:create"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            PlanCreateInput {
                title: "Ship it".into(),
                goal: "Ship it".into(),
                status: Some(PlanStatus::Active),
                policy: None,
                spec_refs: Vec::new(),
            },
        )
        .unwrap();
    let (task_id, _) = store
        .create_task(
            EventMeta {
                id: EventId::new("coord:task:create"),
                ts: 2,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Finish publish".into(),
                status: Some(prism_ir::CoordinationTaskStatus::InProgress),
                assignee: None,
                session: Some(SessionId::new("session:test")),
                worktree_id: None,
                branch_ref: None,
                anchors: Vec::new(),
                depends_on: Vec::new(),
                coordination_depends_on: Vec::new(),
                integrated_depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: WorkspaceRevision::default(),
                spec_refs: Vec::new(),
            },
        )
        .unwrap();

    let prism = Prism::with_history_outcomes_coordination_and_projections(
        Graph::new(),
        HistoryStore::default(),
        OutcomeMemory::default(),
        store.snapshot(),
        ProjectionIndex::default(),
    );

    let task = prism
        .update_native_task_authoritative_only(
            EventMeta {
                id: EventId::new("coord:task:published-expired"),
                ts: 10_000,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            TaskUpdateInput {
                task_id: task_id.clone(),
                kind: None,
                status: Some(prism_ir::CoordinationTaskStatus::Completed),
                published_task_status: Some(None),
                git_execution: Some(TaskGitExecution {
                    status: prism_ir::GitExecutionStatus::CoordinationPublished,
                    integration_mode: prism_ir::GitIntegrationMode::External,
                    integration_status: prism_ir::GitIntegrationStatus::PublishedToBranch,
                    pending_task_status: None,
                    ..TaskGitExecution::default()
                }),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                title: None,
                summary: None,
                anchors: None,
                bindings: None,
                depends_on: None,
                coordination_depends_on: None,
                integrated_depends_on: None,
                acceptance: None,
                validation_refs: None,
                is_abstract: None,
                base_revision: Some(WorkspaceRevision::default()),
                priority: None,
                tags: None,
                completion_context: Some(TaskCompletionContext::default()),
                spec_refs: None,
            },
            WorkspaceRevision::default(),
            10_000,
        )
        .unwrap();

    assert_eq!(task.status, EffectiveTaskStatus::Completed);
    assert_eq!(
        task.task.git_execution.status,
        prism_ir::GitExecutionStatus::CoordinationPublished
    );
    assert_eq!(
        prism.coordination_plan(&plan_id).unwrap().status,
        PlanStatus::Completed
    );
}

#[test]
fn authoritative_only_target_integration_auto_completes_plan() {
    let store = CoordinationStore::new();
    let (plan_id, _) = store
        .create_plan(
            EventMeta {
                id: EventId::new("coord:plan:create"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            PlanCreateInput {
                title: "Land it".into(),
                goal: "Land it".into(),
                status: Some(PlanStatus::Active),
                policy: None,
                spec_refs: Vec::new(),
            },
        )
        .unwrap();
    let (task_id, _) = store
        .create_task(
            EventMeta {
                id: EventId::new("coord:task:create"),
                ts: 2,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Finish landing".into(),
                status: Some(prism_ir::CoordinationTaskStatus::Completed),
                assignee: None,
                session: Some(SessionId::new("session:test")),
                worktree_id: None,
                branch_ref: None,
                anchors: Vec::new(),
                depends_on: Vec::new(),
                coordination_depends_on: Vec::new(),
                integrated_depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: WorkspaceRevision::default(),
                spec_refs: Vec::new(),
            },
        )
        .unwrap();

    let mut snapshot = store.snapshot();
    snapshot.tasks = vec![CoordinationTask {
        id: task_id.clone(),
        plan: plan_id.clone(),
        kind: PlanNodeKind::Edit,
        title: "Finish landing".into(),
        summary: None,
        status: prism_ir::CoordinationTaskStatus::Completed,
        published_task_status: None,
        assignee: None,
        pending_handoff_to: None,
        session: Some(SessionId::new("session:test")),
        lease_holder: None,
        lease_started_at: None,
        lease_refreshed_at: None,
        lease_stale_at: None,
        lease_expires_at: None,
        worktree_id: None,
        branch_ref: None,
        anchors: Vec::new(),
        bindings: prism_ir::PlanBinding::default(),
        depends_on: Vec::new(),
        coordination_depends_on: Vec::new(),
        integrated_depends_on: Vec::new(),
        acceptance: Vec::new(),
        validation_refs: Vec::new(),
        is_abstract: false,
        base_revision: WorkspaceRevision::default(),
        priority: None,
        tags: Vec::new(),
        spec_refs: Vec::new(),
        metadata: serde_json::Value::Null,
        git_execution: TaskGitExecution {
            status: prism_ir::GitExecutionStatus::CoordinationPublished,
            integration_mode: prism_ir::GitIntegrationMode::ManualPr,
            integration_status: prism_ir::GitIntegrationStatus::IntegrationPending,
            ..TaskGitExecution::default()
        },
    }];

    let prism = Prism::with_history_outcomes_coordination_and_projections(
        Graph::new(),
        HistoryStore::default(),
        OutcomeMemory::default(),
        snapshot,
        ProjectionIndex::default(),
    );

    assert_eq!(
        prism.coordination_plan(&plan_id).unwrap().status,
        PlanStatus::Active
    );

    let task = prism
        .update_native_task_authoritative_only(
            EventMeta {
                id: EventId::new("coord:task:integrated"),
                ts: 3,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            TaskUpdateInput {
                task_id,
                kind: None,
                status: None,
                published_task_status: None,
                git_execution: Some(TaskGitExecution {
                    status: prism_ir::GitExecutionStatus::CoordinationPublished,
                    integration_mode: prism_ir::GitIntegrationMode::ManualPr,
                    integration_status: prism_ir::GitIntegrationStatus::IntegratedToTarget,
                    ..TaskGitExecution::default()
                }),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                title: None,
                summary: None,
                anchors: None,
                bindings: None,
                depends_on: None,
                coordination_depends_on: None,
                integrated_depends_on: None,
                acceptance: None,
                validation_refs: None,
                is_abstract: None,
                base_revision: Some(WorkspaceRevision::default()),
                priority: None,
                tags: None,
                completion_context: None,
                spec_refs: None,
            },
            WorkspaceRevision::default(),
            3,
        )
        .unwrap();

    assert_eq!(task.status, EffectiveTaskStatus::Completed);
    assert_eq!(
        task.task.git_execution.integration_status,
        prism_ir::GitIntegrationStatus::IntegratedToTarget
    );
    assert_eq!(
        prism.coordination_plan(&plan_id).unwrap().status,
        PlanStatus::Completed
    );
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
fn symbol_by_id_returns_exact_symbol_without_searching() {
    let mut graph = Graph::new();
    let target = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    graph.add_node(Node {
        id: target.clone(),
        name: "alpha".into(),
        kind: NodeKind::Function,
        file: FileId(1),
        span: Span::line(1),
        language: Language::Rust,
    });

    let prism = Prism::new(graph);
    let symbol = prism
        .symbol_by_id(&target)
        .expect("exact node id lookup should return a symbol");

    assert_eq!(symbol.id(), &target);
    assert_eq!(symbol.name(), "alpha");
}

#[test]
fn search_can_filter_by_kind_and_path() {
    use std::path::Path;

    let mut graph = Graph::new();
    let spec_file = graph.ensure_file(Path::new("workspace/docs/SPEC.md"));
    let source_file = graph.ensure_file(Path::new("workspace/src/spec.rs"));

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
fn concept_lookup_returns_curated_validation_packet() {
    let mut graph = Graph::new();
    graph.add_node(Node {
        id: NodeId::new(
            "demo",
            "demo::impact::Prism::validation_recipe",
            NodeKind::Method,
        ),
        name: "validation_recipe".into(),
        kind: NodeKind::Method,
        file: FileId(1),
        span: Span::line(1),
        language: Language::Rust,
    });
    graph.add_node(Node {
        id: NodeId::new(
            "demo",
            "demo::session_state::SessionState::start_task",
            NodeKind::Method,
        ),
        name: "start_task".into(),
        kind: NodeKind::Method,
        file: FileId(2),
        span: Span::line(1),
        language: Language::Rust,
    });
    graph.add_node(Node {
        id: NodeId::new("demo", "demo::runtime::runtime_status", NodeKind::Function),
        name: "runtime_status".into(),
        kind: NodeKind::Function,
        file: FileId(3),
        span: Span::line(1),
        language: Language::Rust,
    });

    let prism = Prism::new(graph);
    prism.replace_curated_concepts(vec![
        ConceptPacket {
            handle: "concept://validation_pipeline".to_string(),
            canonical_name: "validation_pipeline".to_string(),
            summary: "Curated validation concept.".to_string(),
            aliases: vec!["validation".to_string(), "checks".to_string()],
            confidence: 0.95,
            core_members: vec![NodeId::new(
                "demo",
                "demo::impact::Prism::validation_recipe",
                NodeKind::Method,
            )],
            core_member_lineages: Vec::new(),
            supporting_members: vec![NodeId::new(
                "demo",
                "demo::runtime::runtime_status",
                NodeKind::Function,
            )],
            supporting_member_lineages: Vec::new(),
            likely_tests: Vec::new(),
            likely_test_lineages: Vec::new(),
            evidence: vec!["Curated in test.".to_string()],
            risk_hint: None,
            decode_lenses: vec![ConceptDecodeLens::Validation],
            scope: ConceptScope::Session,
            provenance: ConceptProvenance {
                origin: "test".to_string(),
                kind: "curated_concept".to_string(),
                task_id: None,
            },
            publication: None,
        },
        ConceptPacket {
            handle: "concept://session_lifecycle".to_string(),
            canonical_name: "session_lifecycle".to_string(),
            summary: "Curated session concept.".to_string(),
            aliases: vec!["session".to_string()],
            confidence: 0.9,
            core_members: vec![NodeId::new(
                "demo",
                "demo::session_state::SessionState::start_task",
                NodeKind::Method,
            )],
            core_member_lineages: Vec::new(),
            supporting_members: Vec::new(),
            supporting_member_lineages: Vec::new(),
            likely_tests: Vec::new(),
            likely_test_lineages: Vec::new(),
            evidence: vec!["Curated in test.".to_string()],
            risk_hint: None,
            decode_lenses: vec![ConceptDecodeLens::Open],
            scope: ConceptScope::Session,
            provenance: ConceptProvenance {
                origin: "test".to_string(),
                kind: "curated_concept".to_string(),
                task_id: None,
            },
            publication: None,
        },
    ]);
    let concept = prism.concept("validation").expect("concept should resolve");

    assert_eq!(concept.handle, "concept://validation_pipeline");
    assert!(concept
        .core_members
        .iter()
        .any(|node| node.path.contains("validation_recipe")));
    assert!(prism
        .concept_by_handle("concept://session_lifecycle")
        .is_some());
}

#[test]
fn concept_relation_lookup_returns_direct_neighbors() {
    let prism = Prism::new(Graph::new());
    prism.replace_curated_concepts(vec![
        ConceptPacket {
            handle: "concept://validation_pipeline".to_string(),
            canonical_name: "validation_pipeline".to_string(),
            summary: "Curated validation concept.".to_string(),
            aliases: vec!["validation".to_string()],
            confidence: 0.95,
            core_members: vec![
                NodeId::new("demo", "demo::validation_recipe", NodeKind::Function),
                NodeId::new("demo", "demo::runtime_status", NodeKind::Function),
            ],
            core_member_lineages: Vec::new(),
            supporting_members: Vec::new(),
            supporting_member_lineages: Vec::new(),
            likely_tests: Vec::new(),
            likely_test_lineages: Vec::new(),
            evidence: vec!["Curated in test.".to_string()],
            risk_hint: None,
            decode_lenses: vec![ConceptDecodeLens::Validation],
            scope: ConceptScope::Session,
            provenance: ConceptProvenance {
                origin: "test".to_string(),
                kind: "curated_concept".to_string(),
                task_id: None,
            },
            publication: None,
        },
        ConceptPacket {
            handle: "concept://runtime_surface".to_string(),
            canonical_name: "runtime_surface".to_string(),
            summary: "Curated runtime concept.".to_string(),
            aliases: vec!["runtime".to_string()],
            confidence: 0.9,
            core_members: vec![
                NodeId::new("demo", "demo::runtime_status", NodeKind::Function),
                NodeId::new("demo", "demo::start_task", NodeKind::Function),
            ],
            core_member_lineages: Vec::new(),
            supporting_members: Vec::new(),
            supporting_member_lineages: Vec::new(),
            likely_tests: Vec::new(),
            likely_test_lineages: Vec::new(),
            evidence: vec!["Curated in test.".to_string()],
            risk_hint: None,
            decode_lenses: vec![ConceptDecodeLens::Open],
            scope: ConceptScope::Session,
            provenance: ConceptProvenance {
                origin: "test".to_string(),
                kind: "curated_concept".to_string(),
                task_id: None,
            },
            publication: None,
        },
    ]);
    prism.upsert_concept_relation(ConceptRelation {
        source_handle: "concept://validation_pipeline".to_string(),
        target_handle: "concept://runtime_surface".to_string(),
        kind: ConceptRelationKind::OftenUsedWith,
        confidence: 0.83,
        evidence: vec!["Validation work often moves through runtime state.".to_string()],
        scope: ConceptScope::Session,
        provenance: ConceptProvenance {
            origin: "test".to_string(),
            kind: "concept_relation".to_string(),
            task_id: None,
        },
    });

    let relations = prism.concept_relations_for_handle("concept://validation_pipeline");
    assert_eq!(relations.len(), 1);
    assert_eq!(relations[0].target_handle, "concept://runtime_surface");
    assert_eq!(relations[0].kind, ConceptRelationKind::OftenUsedWith);
}

#[test]
fn concept_health_flags_ambiguous_stale_validation_concepts() {
    let mut graph = Graph::new();
    graph.add_node(Node {
        id: NodeId::new("demo", "demo::validation_recipe", NodeKind::Function),
        name: "validation_recipe".into(),
        kind: NodeKind::Function,
        file: FileId(1),
        span: Span::line(1),
        language: Language::Rust,
    });
    graph.add_node(Node {
        id: NodeId::new("demo", "demo::validation_healthcheck", NodeKind::Function),
        name: "validation_healthcheck".into(),
        kind: NodeKind::Function,
        file: FileId(2),
        span: Span::line(1),
        language: Language::Rust,
    });
    graph.add_node(Node {
        id: NodeId::new("demo", "demo::validation_recipe_test", NodeKind::Function),
        name: "validation_recipe_test".into(),
        kind: NodeKind::Function,
        file: FileId(3),
        span: Span::line(1),
        language: Language::Rust,
    });

    let prism = Prism::new(graph);
    prism.replace_curated_concepts(vec![
        ConceptPacket {
            handle: "concept://validation_pipeline".to_string(),
            canonical_name: "validation_pipeline".to_string(),
            summary: "Validation checks and likely tests.".to_string(),
            aliases: vec!["validation".to_string(), "checks".to_string()],
            confidence: 0.95,
            core_members: vec![NodeId::new(
                "demo",
                "demo::validation_recipe",
                NodeKind::Function,
            )],
            core_member_lineages: Vec::new(),
            supporting_members: Vec::new(),
            supporting_member_lineages: Vec::new(),
            likely_tests: vec![NodeId::new(
                "demo",
                "demo::validation_recipe_test",
                NodeKind::Function,
            )],
            likely_test_lineages: Vec::new(),
            evidence: vec!["Curated in test.".to_string()],
            risk_hint: Some("Validation drift is common here.".to_string()),
            decode_lenses: vec![ConceptDecodeLens::Validation, ConceptDecodeLens::Timeline],
            scope: ConceptScope::Session,
            provenance: ConceptProvenance {
                origin: "test".to_string(),
                kind: "curated_concept".to_string(),
                task_id: None,
            },
            publication: None,
        },
        ConceptPacket {
            handle: "concept://validation_health".to_string(),
            canonical_name: "validation_health".to_string(),
            summary: "Validation-oriented health probes.".to_string(),
            aliases: vec!["validation".to_string()],
            confidence: 0.9,
            core_members: vec![NodeId::new(
                "demo",
                "demo::validation_healthcheck",
                NodeKind::Function,
            )],
            core_member_lineages: Vec::new(),
            supporting_members: Vec::new(),
            supporting_member_lineages: Vec::new(),
            likely_tests: Vec::new(),
            likely_test_lineages: Vec::new(),
            evidence: vec!["Curated in test.".to_string()],
            risk_hint: None,
            decode_lenses: vec![ConceptDecodeLens::Open],
            scope: ConceptScope::Session,
            provenance: ConceptProvenance {
                origin: "test".to_string(),
                kind: "curated_concept".to_string(),
                task_id: None,
            },
            publication: None,
        },
    ]);

    let health = prism
        .concept_health_by_handle("concept://validation_pipeline")
        .expect("health should resolve");

    assert_eq!(
        health.status,
        prism_projections::ConceptHealthStatus::Drifted
    );
    assert!(health.signals.ambiguity_ratio >= 0.6);
    assert!(health.signals.stale_validation_links);
    assert!(health
        .reasons
        .iter()
        .any(|reason| reason.contains("likely tests")));
}

#[test]
fn broad_identifier_search_prefers_code_over_replay_and_lockfile_noise() {
    use std::path::Path;

    let mut graph = Graph::new();
    let planner_file = graph.ensure_file(Path::new("workspace/src/planner.rs"));
    let replay_file = graph.ensure_file(Path::new(
        "workspace/crates/prism-mcp/src/query_replay_cases.rs",
    ));
    let lockfile = graph.ensure_file(Path::new("workspace/www/dashboard/package-lock.json"));

    graph.add_node(Node {
        id: NodeId::new(
            "demo",
            "demo::planner::build_helper_plan",
            NodeKind::Function,
        ),
        name: "build_helper_plan".into(),
        kind: NodeKind::Function,
        file: planner_file,
        span: Span::line(1),
        language: Language::Rust,
    });
    graph.add_node(Node {
        id: NodeId::new(
            "demo",
            "demo::query_replay_cases::assert_repo_helper_bundle",
            NodeKind::Function,
        ),
        name: "assert_repo_helper_bundle".into(),
        kind: NodeKind::Function,
        file: replay_file,
        span: Span::line(1),
        language: Language::Rust,
    });
    graph.add_node(Node {
        id: NodeId::new(
            "demo",
            "demo::document::package_lock_json::packages::node_modules/@babel/helper-globals",
            NodeKind::JsonKey,
        ),
        name: "node_modules/@babel/helper-globals".into(),
        kind: NodeKind::JsonKey,
        file: lockfile,
        span: Span::line(1),
        language: Language::Json,
    });

    let prism = Prism::new(graph);
    let results = prism.search("helper", 5, None, None);

    assert_eq!(results[0].id().path, "demo::planner::build_helper_plan");
    assert!(!results
        .iter()
        .any(|symbol| symbol.id().path.contains("query_replay_cases")));
    assert!(!results
        .iter()
        .any(|symbol| symbol.id().path.contains("@babel/helper-globals")));
}

#[test]
fn broad_identifier_search_suppresses_test_noise_when_non_test_code_exists() {
    use std::path::Path;

    let mut graph = Graph::new();
    let lib_file = graph.ensure_file(Path::new("workspace/src/lib.rs"));
    let helpers_file = graph.ensure_file(Path::new("workspace/src/query_helpers.rs"));

    graph.add_node(Node {
        id: NodeId::new("demo", "demo::build_helper_plan", NodeKind::Function),
        name: "build_helper_plan".into(),
        kind: NodeKind::Function,
        file: lib_file,
        span: Span::line(1),
        language: Language::Rust,
    });
    graph.add_node(Node {
        id: NodeId::new("demo", "demo::query_helpers", NodeKind::Module),
        name: "query_helpers".into(),
        kind: NodeKind::Module,
        file: helpers_file,
        span: Span::line(1),
        language: Language::Rust,
    });
    graph.add_node(Node {
        id: NodeId::new("demo", "demo::tests::helper", NodeKind::Function),
        name: "helper".into(),
        kind: NodeKind::Function,
        file: lib_file,
        span: Span::line(10),
        language: Language::Rust,
    });

    let prism = Prism::new(graph);
    let results = prism.search("helper", 5, None, None);

    assert_eq!(results[0].id().path, "demo::build_helper_plan");
    assert!(results
        .iter()
        .all(|symbol| !symbol.id().path.contains("::tests::")));
}

#[test]
fn broad_identifier_search_prefers_owner_module_over_path_inherited_functions() {
    use std::path::Path;

    let mut graph = Graph::new();
    let helpers_file = graph.ensure_file(Path::new("workspace/src/helpers.rs"));

    graph.add_node(Node {
        id: NodeId::new("demo", "demo::helpers", NodeKind::Module),
        name: "helpers".into(),
        kind: NodeKind::Module,
        file: helpers_file,
        span: Span::line(1),
        language: Language::Rust,
    });
    graph.add_node(Node {
        id: NodeId::new("demo", "demo::helpers::anchor_sort_key", NodeKind::Function),
        name: "anchor_sort_key".into(),
        kind: NodeKind::Function,
        file: helpers_file,
        span: Span::line(3),
        language: Language::Rust,
    });
    graph.add_node(Node {
        id: NodeId::new(
            "demo",
            "demo::helpers::conflict_between",
            NodeKind::Function,
        ),
        name: "conflict_between".into(),
        kind: NodeKind::Function,
        file: helpers_file,
        span: Span::line(7),
        language: Language::Rust,
    });

    let prism = Prism::new(graph);
    let results = prism.search("helper", 5, None, None);

    assert_eq!(results[0].id().path, "demo::helpers");
    assert!(results
        .iter()
        .skip(1)
        .all(|symbol| !matches!(symbol.node().kind, NodeKind::Module)));
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
            execution_context: None,
        },
        trigger: prism_ir::ChangeTrigger::ManualReindex,
        files: vec![FileId(1)],
        previous_path: Some("workspace/src/lib.rs".into()),
        current_path: Some("workspace/src/lib.rs".into()),
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
                execution_context: None,
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
fn outcome_query_filters_expand_node_focus_with_additional_filters() {
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
    let task = TaskId::new("task:alpha");
    outcomes
        .store_event(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:filter:1"),
                ts: 5,
                actor: EventActor::System,
                correlation: Some(task.clone()),
                causation: None,
                execution_context: None,
            },
            anchors: vec![AnchorRef::Node(alpha.clone())],
            kind: OutcomeKind::FailureObserved,
            result: OutcomeResult::Failure,
            summary: "system failure".into(),
            evidence: Vec::new(),
            metadata: serde_json::Value::Null,
        })
        .unwrap();
    outcomes
        .store_event(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:filter:2"),
                ts: 12,
                actor: EventActor::Agent,
                correlation: Some(task.clone()),
                causation: None,
                execution_context: None,
            },
            anchors: vec![AnchorRef::Node(alpha.clone())],
            kind: OutcomeKind::FailureObserved,
            result: OutcomeResult::Failure,
            summary: "agent failure".into(),
            evidence: Vec::new(),
            metadata: serde_json::Value::Null,
        })
        .unwrap();

    let prism = Prism::with_history_and_outcomes(graph, history, outcomes);
    let events = prism.query_outcomes(&OutcomeRecallQuery {
        anchors: vec![AnchorRef::Node(alpha)],
        task: Some(task),
        kinds: Some(vec![OutcomeKind::FailureObserved]),
        result: Some(OutcomeResult::Failure),
        actor: Some(EventActor::Agent),
        since: Some(10),
        limit: 10,
    });

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].summary, "agent failure");

    let legacy_events = prism.query_outcomes(&OutcomeRecallQuery {
        anchors: vec![AnchorRef::Node(NodeId::new(
            "demo",
            "demo::alpha",
            NodeKind::Function,
        ))],
        task: Some(TaskId::new("task:alpha")),
        kinds: Some(vec![OutcomeKind::FailureObserved]),
        result: Some(OutcomeResult::Failure),
        actor: Some(EventActor::Agent.canonical_identity_actor()),
        since: Some(10),
        limit: 10,
    });

    assert_eq!(legacy_events.len(), 1);
    assert_eq!(legacy_events[0].summary, "agent failure");
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
                execution_context: None,
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
            execution_context: None,
        },
        trigger: ChangeTrigger::ManualReindex,
        files: vec![FileId(1)],
        previous_path: Some("workspace/src/lib.rs".into()),
        current_path: Some("workspace/src/lib.rs".into()),
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
                    fingerprint: prism_ir::SymbolFingerprint::with_parts(10, Some(20), None, None),
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
                    fingerprint: prism_ir::SymbolFingerprint::with_parts(10, Some(21), None, None),
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
                    fingerprint: prism_ir::SymbolFingerprint::with_parts(11, Some(30), None, None),
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
                    fingerprint: prism_ir::SymbolFingerprint::with_parts(11, Some(31), None, None),
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
                execution_context: None,
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
                execution_context: None,
            },
            PlanCreateInput {
                title: "Coordinate alpha".into(),
                goal: "Coordinate alpha".into(),
                status: None,
                policy: None,
                spec_refs: Vec::new(),
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
                execution_context: None,
            },
            TaskCreateInput {
                plan_id,
                title: "Edit alpha".into(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: Some(SessionId::new("session:a")),
                worktree_id: None,
                branch_ref: None,
                anchors: vec![AnchorRef::Node(alpha.clone())],
                depends_on: Vec::new(),
                coordination_depends_on: Vec::new(),
                integrated_depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                spec_refs: Vec::new(),
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
                execution_context: None,
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
                current_revision: WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                agent: None,
                worktree_id: None,
                branch_ref: None,
            },
        )
        .unwrap();

    let prism = Prism::with_history_outcomes_coordination_and_projections(
        graph,
        history,
        OutcomeMemory::new(),
        coordination.snapshot(),
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
    assert!(simulated.iter().any(|conflict| {
        conflict.overlap_kinds.iter().any(|kind| {
            matches!(
                kind,
                prism_ir::ConflictOverlapKind::Node
                    | prism_ir::ConflictOverlapKind::Lineage
                    | prism_ir::ConflictOverlapKind::File
            )
        })
    }));
}

#[test]
fn plans_contains_filter_matches_singular_and_plural_plan_terms() {
    let graph = Graph::new();
    let history = HistoryStore::new();
    let outcomes = OutcomeMemory::new();
    let coordination = CoordinationStore::new();
    coordination
        .create_plan(
            EventMeta {
                id: EventId::new("coord:plan:bottleneck"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            PlanCreateInput {
                title: "Eliminate the remaining performance bottleneck".into(),
                goal: "Eliminate the remaining performance bottleneck".into(),
                status: None,
                policy: None,
                spec_refs: Vec::new(),
            },
        )
        .unwrap();
    let prism = Prism::with_history_outcomes_coordination_and_projections(
        graph,
        history,
        outcomes,
        coordination.snapshot(),
        ProjectionIndex::default(),
    );

    let plans = prism.plans(None, None, Some("bottlenecks"));
    assert_eq!(plans.len(), 1);
    assert!(plans[0].title.to_ascii_lowercase().contains("bottleneck"));
}

#[test]
fn continuity_reads_native_runtime_state_before_coordination_projection() {
    let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    let mut graph = Graph::new();
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
    let coordination = CoordinationStore::new();
    let (plan_id, _) = coordination
        .create_plan(
            EventMeta {
                id: EventId::new("coord:plan:runtime"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            PlanCreateInput {
                title: "Continuity runtime".into(),
                goal: "Continuity runtime".into(),
                status: None,
                policy: None,
                spec_refs: Vec::new(),
            },
        )
        .unwrap();
    let (task_id, _) = coordination
        .create_task(
            EventMeta {
                id: EventId::new("coord:task:runtime"),
                ts: 2,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Task A".into(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                anchors: vec![AnchorRef::Node(alpha.clone())],
                depends_on: Vec::new(),
                coordination_depends_on: Vec::new(),
                integrated_depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: WorkspaceRevision::default(),
                spec_refs: Vec::new(),
            },
        )
        .unwrap();

    let prism = Prism::with_history_outcomes_coordination_and_projections(
        graph,
        history,
        OutcomeMemory::new(),
        coordination.snapshot(),
        ProjectionIndex::default(),
    );

    let mut runtime_snapshot = prism.coordination_snapshot();
    runtime_snapshot
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .expect("runtime task should exist")
        .title = "Task A runtime".into();
    runtime_snapshot
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .expect("runtime task should exist")
        .depends_on = vec![prism_ir::CoordinationTaskId::new("coord-task:missing")];
    runtime_snapshot.claims.push(WorkClaim {
        id: prism_ir::ClaimId::new("claim:runtime"),
        holder: SessionId::new("session:runtime"),
        agent: Some(prism_ir::AgentId::new("agent-runtime")),
        lease_holder: None,
        worktree_id: None,
        branch_ref: None,
        task: Some(task_id.clone()),
        anchors: vec![AnchorRef::Node(alpha.clone())],
        capability: prism_ir::Capability::Edit,
        mode: prism_ir::ClaimMode::SoftExclusive,
        since: 3,
        refreshed_at: None,
        stale_at: None,
        expires_at: 30,
        status: prism_ir::ClaimStatus::Active,
        base_revision: WorkspaceRevision::default(),
    });
    runtime_snapshot.artifacts.push(Artifact {
        id: prism_ir::ArtifactId::new("artifact:runtime"),
        task: task_id.clone(),
        worktree_id: None,
        branch_ref: None,
        anchors: vec![AnchorRef::Node(alpha.clone())],
        base_revision: WorkspaceRevision::default(),
        diff_ref: None,
        status: prism_ir::ArtifactStatus::Proposed,
        evidence: Vec::new(),
        reviews: Vec::new(),
        required_validations: Vec::new(),
        validated_checks: Vec::new(),
        risk_score: None,
    });
    prism.replace_coordination_snapshot(runtime_snapshot);

    assert_eq!(prism.coordination_snapshot().claims.len(), 1);
    assert_eq!(prism.coordination_snapshot().artifacts.len(), 1);
    assert_eq!(
        prism
            .coordination_snapshot()
            .tasks
            .into_iter()
            .find(|task| task.id == task_id)
            .expect("runtime task should exist")
            .title,
        "Task A runtime"
    );
    assert_eq!(
        prism
            .coordination_task(&task_id)
            .expect("runtime task should exist")
            .title,
        "Task A runtime"
    );
    assert_eq!(prism.claims(&[AnchorRef::Node(alpha.clone())], 10).len(), 1);
    assert_eq!(prism.artifacts(&task_id).len(), 1);
    assert_eq!(
        prism
            .coordination_artifact(&prism_ir::ArtifactId::new("artifact:runtime"))
            .expect("runtime artifact should exist")
            .task,
        task_id
    );
}

#[test]
fn claim_reads_and_simulation_respect_worktree_scope() {
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
    let prism = Prism::with_history_outcomes_coordination_and_projections(
        graph,
        HistoryStore::new(),
        OutcomeMemory::new(),
        CoordinationSnapshot::default(),
        ProjectionIndex::default(),
    );
    prism.set_coordination_context(Some(CoordinationPersistContext {
        repo_id: "repo:test".into(),
        worktree_id: "worktree:a".into(),
        branch_ref: Some("refs/heads/a".into()),
        session_id: None,
        instance_id: Some("instance:test".into()),
    }));

    let mut runtime_snapshot = prism.coordination_snapshot();
    runtime_snapshot.claims.push(WorkClaim {
        id: prism_ir::ClaimId::new("claim:a"),
        holder: SessionId::new("session:a"),
        agent: None,
        lease_holder: None,
        worktree_id: Some("worktree:a".into()),
        branch_ref: Some("refs/heads/a".into()),
        task: None,
        anchors: vec![AnchorRef::Node(alpha.clone())],
        capability: prism_ir::Capability::Edit,
        mode: prism_ir::ClaimMode::HardExclusive,
        since: 1,
        refreshed_at: None,
        stale_at: None,
        expires_at: 100,
        status: prism_ir::ClaimStatus::Active,
        base_revision: WorkspaceRevision::default(),
    });
    runtime_snapshot.claims.push(WorkClaim {
        id: prism_ir::ClaimId::new("claim:b"),
        holder: SessionId::new("session:b"),
        agent: None,
        lease_holder: None,
        worktree_id: Some("worktree:b".into()),
        branch_ref: Some("refs/heads/b".into()),
        task: None,
        anchors: vec![AnchorRef::Node(alpha.clone())],
        capability: prism_ir::Capability::Edit,
        mode: prism_ir::ClaimMode::HardExclusive,
        since: 1,
        refreshed_at: None,
        stale_at: None,
        expires_at: 100,
        status: prism_ir::ClaimStatus::Active,
        base_revision: WorkspaceRevision::default(),
    });
    prism.replace_coordination_snapshot(runtime_snapshot);

    let claims = prism.claims(&[AnchorRef::Node(alpha.clone())], 10);
    assert_eq!(claims.len(), 1);
    assert_eq!(claims[0].id.0, "claim:a");

    let conflicts = prism.simulate_claim(
        &SessionId::new("session:new"),
        &[AnchorRef::Node(alpha)],
        prism_ir::Capability::Edit,
        Some(prism_ir::ClaimMode::HardExclusive),
        None,
        10,
    );
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].blocking_claims[0].0, "claim:a");
}

#[test]
fn artifact_reads_and_pending_reviews_respect_worktree_scope() {
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
    let prism = Prism::with_history_outcomes_coordination_and_projections(
        graph,
        HistoryStore::new(),
        OutcomeMemory::new(),
        CoordinationSnapshot::default(),
        ProjectionIndex::default(),
    );
    prism.set_coordination_context(Some(CoordinationPersistContext {
        repo_id: "repo:test".into(),
        worktree_id: "worktree:a".into(),
        branch_ref: Some("refs/heads/a".into()),
        session_id: None,
        instance_id: Some("instance:test".into()),
    }));

    let seeded = CoordinationStore::new();
    let (plan_id, _) = seeded
        .create_plan(
            EventMeta {
                id: EventId::new("coord:plan:artifact-scope"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            PlanCreateInput {
                title: "Scoped artifact reviews".into(),
                goal: "Scoped artifact reviews".into(),
                status: None,
                policy: Some(CoordinationPolicy::default()),
                spec_refs: Vec::new(),
            },
        )
        .unwrap();
    let (task_id, _) = seeded
        .create_task(
            EventMeta {
                id: EventId::new("coord:task:artifact-scope"),
                ts: 2,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Edit alpha".into(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                anchors: vec![AnchorRef::Node(alpha.clone())],
                depends_on: Vec::new(),
                coordination_depends_on: Vec::new(),
                integrated_depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: WorkspaceRevision::default(),
                spec_refs: Vec::new(),
            },
        )
        .unwrap();
    let mut runtime_snapshot = seeded.snapshot();
    let review_id = prism_ir::ReviewId::new("review:a");
    runtime_snapshot.artifacts.push(Artifact {
        id: prism_ir::ArtifactId::new("artifact:a"),
        task: task_id.clone(),
        worktree_id: Some("worktree:a".into()),
        branch_ref: Some("refs/heads/a".into()),
        anchors: vec![AnchorRef::Node(alpha.clone())],
        base_revision: WorkspaceRevision::default(),
        diff_ref: Some("patch:a".into()),
        status: prism_ir::ArtifactStatus::Proposed,
        evidence: Vec::new(),
        reviews: vec![review_id.clone()],
        required_validations: Vec::new(),
        validated_checks: Vec::new(),
        risk_score: None,
    });
    runtime_snapshot.artifacts.push(Artifact {
        id: prism_ir::ArtifactId::new("artifact:b"),
        task: task_id.clone(),
        worktree_id: Some("worktree:b".into()),
        branch_ref: Some("refs/heads/b".into()),
        anchors: vec![AnchorRef::Node(alpha.clone())],
        base_revision: WorkspaceRevision::default(),
        diff_ref: Some("patch:b".into()),
        status: prism_ir::ArtifactStatus::Proposed,
        evidence: Vec::new(),
        reviews: Vec::new(),
        required_validations: Vec::new(),
        validated_checks: Vec::new(),
        risk_score: None,
    });
    runtime_snapshot
        .reviews
        .push(prism_coordination::ArtifactReview {
            id: review_id.clone(),
            artifact: prism_ir::ArtifactId::new("artifact:a"),
            verdict: prism_ir::ReviewVerdict::Approved,
            meta: EventMeta {
                id: EventId::new("coord:review:artifact-scope"),
                ts: 3,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            summary: "LGTM".into(),
        });
    prism.replace_coordination_snapshot(runtime_snapshot);

    let artifacts = prism.artifacts(&task_id);
    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0].id.0, "artifact:a");
    assert_eq!(prism.pending_reviews(Some(&plan_id)).len(), 1);
    assert_eq!(
        prism
            .coordination_artifact(&prism_ir::ArtifactId::new("artifact:a"))
            .map(|artifact| artifact.id.0),
        Some("artifact:a".into())
    );
    assert!(prism
        .coordination_artifact(&prism_ir::ArtifactId::new("artifact:b"))
        .is_none());
    assert_eq!(
        prism
            .coordination_review(&review_id)
            .map(|review| review.id),
        Some(review_id)
    );
}

#[test]
fn task_evidence_status_aggregates_artifacts_reviews_and_blockers() {
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
    let coordination = CoordinationStore::new();
    let (plan_id, _) = coordination
        .create_plan(
            EventMeta {
                id: EventId::new("coord:plan:task-evidence-status"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            PlanCreateInput {
                title: "Task evidence status".into(),
                goal: "Aggregate artifact and review posture".into(),
                status: None,
                policy: Some(CoordinationPolicy {
                    review_required_above_risk_score: Some(0.0),
                    require_validation_for_completion: true,
                    ..CoordinationPolicy::default()
                }),
                spec_refs: Vec::new(),
            },
        )
        .unwrap();
    let (task_id, _) = coordination
        .create_task(
            EventMeta {
                id: EventId::new("coord:task:task-evidence-status"),
                ts: 2,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Implement alpha".into(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                anchors: vec![AnchorRef::Node(alpha.clone())],
                depends_on: Vec::new(),
                coordination_depends_on: Vec::new(),
                integrated_depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: WorkspaceRevision::default(),
                spec_refs: Vec::new(),
            },
        )
        .unwrap();
    let artifact_id = prism_ir::ArtifactId::new("artifact:evidence-status");
    let review_id = prism_ir::ReviewId::new("review:evidence-status");
    let mut snapshot = coordination.snapshot();
    snapshot.artifacts.push(Artifact {
        id: artifact_id.clone(),
        task: task_id.clone(),
        worktree_id: None,
        branch_ref: None,
        anchors: Vec::new(),
        base_revision: WorkspaceRevision::default(),
        diff_ref: Some("patch:evidence".into()),
        status: prism_ir::ArtifactStatus::InReview,
        evidence: Vec::new(),
        reviews: vec![review_id.clone()],
        required_validations: vec!["test:alpha".into()],
        validated_checks: Vec::new(),
        risk_score: Some(0.8),
    });
    snapshot.reviews.push(prism_coordination::ArtifactReview {
        id: review_id.clone(),
        artifact: artifact_id.clone(),
        verdict: prism_ir::ReviewVerdict::ChangesRequested,
        summary: "needs changes".into(),
        meta: EventMeta {
            id: EventId::new("coord:review:evidence-status"),
            ts: 3,
            actor: EventActor::Agent,
            correlation: None,
            causation: None,
            execution_context: None,
        },
    });

    let prism = Prism::with_history_outcomes_coordination_and_projections(
        graph,
        HistoryStore::new(),
        OutcomeMemory::new(),
        snapshot,
        ProjectionIndex::default(),
    );

    let evidence = prism
        .task_evidence_status(&task_id, 5)
        .expect("task evidence status");
    assert_eq!(evidence.task_id, task_id);
    assert_eq!(evidence.artifacts.len(), 1);
    assert_eq!(evidence.pending_review_count, 1);
    assert_eq!(evidence.rejected_artifact_count, 1);
    assert!(evidence.review_required);
    assert!(!evidence.has_approved_artifact);
    assert_eq!(evidence.missing_validations, vec!["test:alpha"]);
    assert_eq!(
        evidence.artifacts[0].latest_review_verdict,
        Some(prism_ir::ReviewVerdict::ChangesRequested)
    );
    assert!(evidence.artifacts[0].pending_review);

    let review_status = prism
        .task_review_status(&task_id, 5)
        .expect("task review status");
    assert_eq!(review_status.pending_review_count, 1);
    assert_eq!(review_status.rejected_artifact_count, 1);
}

#[test]
fn ready_tasks_and_handoff_acceptance_respect_worktree_scope() {
    let coordination = CoordinationStore::new();
    let (plan_id, _) = coordination
        .create_plan(
            EventMeta {
                id: EventId::new("coord:plan:worktree-ready"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            PlanCreateInput {
                title: "Scoped ready work".into(),
                goal: "Scoped ready work".into(),
                status: Some(prism_ir::PlanStatus::Active),
                policy: None,
                spec_refs: Vec::new(),
            },
        )
        .unwrap();
    let prism = Prism::with_history_outcomes_coordination_and_projections(
        Graph::new(),
        HistoryStore::new(),
        OutcomeMemory::new(),
        coordination.snapshot(),
        ProjectionIndex::default(),
    );

    prism.set_coordination_context(Some(CoordinationPersistContext {
        repo_id: "repo:test".into(),
        worktree_id: "worktree:a".into(),
        branch_ref: Some("refs/heads/a".into()),
        session_id: None,
        instance_id: Some("instance:test".into()),
    }));
    let task = prism
        .create_native_task(
            EventMeta {
                id: EventId::new("coord:task:worktree-ready"),
                ts: 2,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Edit alpha".into(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: Some(prism_ir::AgentId::new("agent-a")),
                session: Some(SessionId::new("session:a")),
                worktree_id: None,
                branch_ref: None,
                anchors: Vec::new(),
                depends_on: Vec::new(),
                coordination_depends_on: Vec::new(),
                integrated_depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: WorkspaceRevision::default(),
                spec_refs: Vec::new(),
            },
        )
        .unwrap();
    assert_eq!(task.task.worktree_id.as_deref(), Some("worktree:a"));
    assert_eq!(prism.ready_tasks(&plan_id, 10).len(), 1);

    prism.set_coordination_context(Some(CoordinationPersistContext {
        repo_id: "repo:test".into(),
        worktree_id: "worktree:b".into(),
        branch_ref: Some("refs/heads/b".into()),
        session_id: None,
        instance_id: Some("instance:test".into()),
    }));
    assert!(prism.ready_tasks(&plan_id, 10).is_empty());

    prism.set_coordination_context(Some(CoordinationPersistContext {
        repo_id: "repo:test".into(),
        worktree_id: "worktree:a".into(),
        branch_ref: Some("refs/heads/a".into()),
        session_id: None,
        instance_id: Some("instance:test".into()),
    }));
    let handoff = prism
        .request_native_handoff_transaction(
            EventMeta {
                id: EventId::new("coord:handoff:worktree-ready"),
                ts: 3,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            HandoffInput {
                task_id: CoordinationTaskId::new(task.task.id.0.clone()),
                to_agent: Some(prism_ir::AgentId::new("agent-b")),
                summary: "handoff".into(),
                base_revision: WorkspaceRevision::default(),
            },
            WorkspaceRevision::default(),
        )
        .unwrap();
    assert_eq!(handoff.task_id, CoordinationTaskId::new(task.task.id.0.clone()));
    assert!(handoff.transaction.commit.event_count >= 1);
    assert_eq!(
        handoff.transaction.authority_version.last_event_id,
        handoff.transaction.commit.last_event_id
    );

    prism.set_coordination_context(Some(CoordinationPersistContext {
        repo_id: "repo:test".into(),
        worktree_id: "worktree:b".into(),
        branch_ref: Some("refs/heads/b".into()),
        session_id: None,
        instance_id: Some("instance:test".into()),
    }));
    let accepted = prism
        .accept_native_handoff_transaction(
            EventMeta {
                id: EventId::new("coord:handoff-accept:worktree-ready"),
                ts: 4,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            prism_coordination::HandoffAcceptInput {
                task_id: CoordinationTaskId::new(task.task.id.0.clone()),
                agent: Some(prism_ir::AgentId::new("agent-b")),
                worktree_id: None,
                branch_ref: None,
            },
        )
        .unwrap();
    assert_eq!(accepted.task_id, CoordinationTaskId::new(task.task.id.0.clone()));
    assert!(accepted.transaction.commit.event_count >= 1);
    assert_eq!(
        accepted.transaction.authority_version.last_event_id,
        accepted.transaction.commit.last_event_id
    );
    let accepted = prism
        .coordination_task(&accepted.task_id)
        .expect("accepted task should remain queryable");
    assert_eq!(accepted.worktree_id.as_deref(), Some("worktree:b"));
    let projected = prism
        .coordination_task(&CoordinationTaskId::new(task.task.id.0.clone()))
        .expect("accepted task should remain queryable");
    assert_eq!(projected.worktree_id.as_deref(), Some("worktree:b"));
    assert_eq!(projected.status, prism_ir::CoordinationTaskStatus::Ready);
    assert_eq!(prism.ready_tasks(&plan_id, 10).len(), 1);

    prism.set_coordination_context(Some(CoordinationPersistContext {
        repo_id: "repo:test".into(),
        worktree_id: "worktree:a".into(),
        branch_ref: Some("refs/heads/a".into()),
        session_id: None,
        instance_id: Some("instance:test".into()),
    }));
    assert!(prism.ready_tasks(&plan_id, 10).is_empty());
}

#[test]
fn spec_sync_create_helpers_attach_typed_spec_refs() {
    let prism = Prism::with_history_outcomes_coordination_and_projections(
        Graph::new(),
        HistoryStore::new(),
        OutcomeMemory::new(),
        CoordinationSnapshot::default(),
        ProjectionIndex::default(),
    );

    let plan = prism
        .create_native_plan_from_spec_transaction(
            EventMeta {
                id: EventId::new("coord:plan:spec-sync"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            NativeSpecPlanCreateInput {
                title: "Ship alpha".into(),
                goal: "Ship alpha".into(),
                status: Some(PlanStatus::Active),
                policy: None,
                scheduling: None,
                spec_ref: CoordinationSpecRef {
                    spec_id: "spec:alpha".into(),
                    source_path: ".prism/specs/2026-04-09-alpha.md".into(),
                    source_revision: Some("rev-plan".into()),
                },
            },
        )
        .expect("spec-linked plan create should succeed");

    let task = prism
        .create_native_task_from_spec_transaction(
            EventMeta {
                id: EventId::new("coord:task:spec-sync"),
                ts: 2,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            NativeSpecTaskCreateInput {
                task: TaskCreateInput {
                    plan_id: plan.plan_id.clone(),
                    title: "Implement alpha".into(),
                    status: Some(prism_ir::CoordinationTaskStatus::Ready),
                    assignee: None,
                    session: None,
                    worktree_id: None,
                    branch_ref: None,
                    anchors: Vec::new(),
                    depends_on: Vec::new(),
                    coordination_depends_on: Vec::new(),
                    integrated_depends_on: Vec::new(),
                    acceptance: Vec::new(),
                    base_revision: WorkspaceRevision::default(),
                    spec_refs: Vec::new(),
                },
                spec_ref: CoordinationTaskSpecRef {
                    spec_id: "spec:alpha".into(),
                    source_path: ".prism/specs/2026-04-09-alpha.md".into(),
                    source_revision: Some("rev-task".into()),
                    sync_kind: "task".into(),
                    covered_checklist_items: vec!["spec:alpha::checklist::item-1".into()],
                    covered_sections: Vec::new(),
                },
            },
        )
        .expect("spec-linked task create should succeed");

    let snapshot = prism.coordination_snapshot();
    let plan_record = snapshot
        .plans
        .iter()
        .find(|candidate| candidate.id == plan.plan_id)
        .expect("created plan should exist");
    assert_eq!(plan_record.spec_refs.len(), 1);
    assert_eq!(plan_record.spec_refs[0].spec_id, "spec:alpha");
    assert_eq!(
        plan_record.spec_refs[0].source_revision.as_deref(),
        Some("rev-plan")
    );

    let task_record = snapshot
        .tasks
        .iter()
        .find(|candidate| candidate.id == task.task_id)
        .expect("created task should exist");
    assert_eq!(task_record.spec_refs.len(), 1);
    assert_eq!(task_record.spec_refs[0].spec_id, "spec:alpha");
    assert_eq!(
        task_record.spec_refs[0].covered_checklist_items,
        vec!["spec:alpha::checklist::item-1"]
    );
}

#[test]
fn spec_sync_helpers_refresh_coverage_and_sync_provenance_end_to_end() {
    use prism_spec::{
        refresh_spec_materialization, MaterializedSpecQueryEngine, SpecQueryEngine,
        SpecQueryLookup, SqliteSpecMaterializedStore,
    };
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static NEXT_TEMP_REPO: AtomicU64 = AtomicU64::new(0);

    fn temp_repo(label: &str) -> PathBuf {
        let nonce = NEXT_TEMP_REPO.fetch_add(1, Ordering::Relaxed);
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("prism-query-spec-sync-{label}-{unique}-{nonce}"));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
        fs::write(root.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        root
    }

    let prism = Prism::with_history_outcomes_coordination_and_projections(
        Graph::new(),
        HistoryStore::new(),
        OutcomeMemory::new(),
        CoordinationSnapshot::default(),
        ProjectionIndex::default(),
    );
    let spec_root = temp_repo("end-to-end");
    let spec_path = ".prism/specs/2026-04-09-alpha.md";
    fs::create_dir_all(spec_root.join(".prism/specs")).unwrap();
    fs::write(
        spec_root.join(spec_path),
        "---\n\
id: spec:alpha\n\
title: Alpha\n\
status: in_progress\n\
created: 2026-04-09\n\
---\n\
\n\
- [ ] implement core flow <!-- id: item-1 -->\n\
- [ ] validate rollout <!-- id: item-2 -->\n",
    )
    .unwrap();

    let plan = prism
        .create_native_plan_from_spec_transaction(
            EventMeta {
                id: EventId::new("coord:plan:spec-sync:e2e"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            NativeSpecPlanCreateInput {
                title: "Ship alpha".into(),
                goal: "Ship alpha".into(),
                status: Some(PlanStatus::Active),
                policy: None,
                scheduling: None,
                spec_ref: CoordinationSpecRef {
                    spec_id: "spec:alpha".into(),
                    source_path: spec_path.into(),
                    source_revision: Some("rev-plan".into()),
                },
            },
        )
        .expect("spec-linked plan create should succeed");
    let task = prism
        .create_native_task_from_spec_transaction(
            EventMeta {
                id: EventId::new("coord:task:spec-sync:e2e"),
                ts: 2,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            NativeSpecTaskCreateInput {
                task: TaskCreateInput {
                    plan_id: plan.plan_id.clone(),
                    title: "Implement alpha".into(),
                    status: Some(prism_ir::CoordinationTaskStatus::Ready),
                    assignee: None,
                    session: None,
                    worktree_id: None,
                    branch_ref: None,
                    anchors: Vec::new(),
                    depends_on: Vec::new(),
                    coordination_depends_on: Vec::new(),
                    integrated_depends_on: Vec::new(),
                    acceptance: Vec::new(),
                    base_revision: WorkspaceRevision::default(),
                    spec_refs: Vec::new(),
                },
                spec_ref: CoordinationTaskSpecRef {
                    spec_id: "spec:alpha".into(),
                    source_path: spec_path.into(),
                    source_revision: Some("rev-task".into()),
                    sync_kind: "task".into(),
                    covered_checklist_items: vec!["spec:alpha::checklist::item-1".into()],
                    covered_sections: Vec::new(),
                },
            },
        )
        .expect("spec-linked task create should succeed");

    let store = SqliteSpecMaterializedStore::new(&spec_root.join(".tmp/spec-materialized.db"));
    let refresh =
        refresh_spec_materialization(&store, &spec_root, Some(prism.coordination_snapshot()))
            .expect("spec refresh should succeed");
    assert!(refresh.diagnostics.is_empty());

    let engine = MaterializedSpecQueryEngine::new(&store);

    match engine.coverage("spec:alpha").unwrap() {
        SpecQueryLookup::Found(view) => {
            assert_eq!(view.records.len(), 2);
            assert_eq!(
                view.records
                    .iter()
                    .map(|record| record.coverage_kind.as_str())
                    .collect::<Vec<_>>(),
                vec!["represented", "uncovered"]
            );
            assert_eq!(
                view.records[0].checklist_item_id,
                "spec:alpha::checklist::item-1"
            );
            assert_eq!(
                view.records[0].coordination_ref.as_deref(),
                Some(task.task_id.0.as_str())
            );
            assert_eq!(
                view.records[1].checklist_item_id,
                "spec:alpha::checklist::item-2"
            );
            assert_eq!(view.records[1].coordination_ref, None);
        }
        SpecQueryLookup::NotFound => panic!("expected coverage view"),
    }

    match engine.sync_provenance("spec:alpha").unwrap() {
        SpecQueryLookup::Found(view) => {
            assert_eq!(view.records.len(), 2);
            assert_eq!(view.records[0].target_coordination_ref, task.task_id.0);
            assert_eq!(view.records[0].sync_kind, "task");
            assert_eq!(
                view.records[0].covered_checklist_items,
                vec!["spec:alpha::checklist::item-1"]
            );
            assert_eq!(view.records[1].target_coordination_ref, plan.plan_id.0);
            assert_eq!(view.records[1].sync_kind, "plan");
        }
        SpecQueryLookup::NotFound => panic!("expected sync provenance view"),
    }

    match engine.sync_brief("spec:alpha").unwrap() {
        SpecQueryLookup::Found(view) => {
            assert_eq!(view.required_checklist_items.len(), 2);
            assert_eq!(view.coverage.len(), 2);
            assert_eq!(view.linked_coordination_refs.len(), 2);
        }
        SpecQueryLookup::NotFound => panic!("expected sync brief"),
    }
}

#[test]
fn ready_tasks_for_executor_filters_by_executor_policy() {
    let graph = Graph::new();
    let history = HistoryStore::new();
    let outcomes = OutcomeMemory::new();
    let plan_id = PlanId::new("plan:executor-routing");
    let matching_task_id = CoordinationTaskId::new("coord-task:match");
    let pinned_elsewhere_task_id = CoordinationTaskId::new("coord-task:pinned-elsewhere");
    let prism = Prism::with_history_outcomes_coordination_and_projections(
        graph,
        history,
        outcomes,
        CoordinationSnapshot {
            plans: vec![Plan {
                id: plan_id.clone(),
                goal: "Filter runnable work".into(),
                title: "Filter runnable work".into(),
                status: PlanStatus::Active,
                policy: CoordinationPolicy::default(),
                scope: PlanScope::Repo,
                kind: PlanKind::TaskExecution,
                revision: 0,
                scheduling: PlanScheduling::default(),
                tags: Vec::new(),
                created_from: None,
                spec_refs: Vec::new(),
                metadata: serde_json::Value::Null,
            }],
            tasks: vec![
                CoordinationTask {
                    id: matching_task_id.clone(),
                    plan: plan_id.clone(),
                    kind: PlanNodeKind::Edit,
                    title: "Runs here".into(),
                    summary: None,
                    status: prism_ir::CoordinationTaskStatus::Ready,
                    published_task_status: None,
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
                    bindings: prism_ir::PlanBinding::default(),
                    depends_on: Vec::new(),
                    coordination_depends_on: Vec::new(),
                    integrated_depends_on: Vec::new(),
                    acceptance: Vec::new(),
                    validation_refs: Vec::new(),
                    is_abstract: false,
                    base_revision: WorkspaceRevision::default(),
                    priority: None,
                    tags: Vec::new(),
                    spec_refs: Vec::new(),
                    metadata: json!({
                        "executor": {
                            "executorClass": "worktree_executor",
                            "targetLabel": "agent-a",
                            "allowedPrincipals": ["worktree:a"]
                        }
                    }),
                    git_execution: TaskGitExecution::default(),
                },
                CoordinationTask {
                    id: pinned_elsewhere_task_id.clone(),
                    plan: plan_id.clone(),
                    kind: PlanNodeKind::Edit,
                    title: "Pinned elsewhere".into(),
                    summary: None,
                    status: prism_ir::CoordinationTaskStatus::Ready,
                    published_task_status: None,
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
                    bindings: prism_ir::PlanBinding::default(),
                    depends_on: Vec::new(),
                    coordination_depends_on: Vec::new(),
                    integrated_depends_on: Vec::new(),
                    acceptance: Vec::new(),
                    validation_refs: Vec::new(),
                    is_abstract: false,
                    base_revision: WorkspaceRevision::default(),
                    priority: None,
                    tags: Vec::new(),
                    spec_refs: Vec::new(),
                    metadata: json!({
                        "executor": {
                            "executorClass": "worktree_executor",
                            "targetLabel": "agent-a",
                            "allowedPrincipals": ["worktree:b"]
                        }
                    }),
                    git_execution: TaskGitExecution::default(),
                },
            ],
            claims: Vec::new(),
            artifacts: Vec::new(),
            reviews: Vec::new(),
            events: Vec::new(),
            next_plan: 1,
            next_task: 1,
            next_claim: 0,
            next_artifact: 0,
            next_review: 0,
        },
        ProjectionIndex::default(),
    );

    let caller = TaskExecutorCaller::new(
        ExecutorClass::WorktreeExecutor,
        Some("agent-a".into()),
        Some(PrincipalId::new("worktree:a")),
    );
    let ready = prism.ready_tasks_for_executor(&plan_id, 10, &caller);
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].id, matching_task_id);

    let human = TaskExecutorCaller::new(
        ExecutorClass::Human,
        Some("owner".into()),
        Some(PrincipalId::new("human:owner")),
    );
    assert!(prism
        .ready_tasks_for_executor(&plan_id, 10, &human)
        .is_empty());
}

#[test]
fn published_plan_unbound_tasks_stay_actionable_across_unrelated_graph_drift() {
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
    history.seed_nodes([alpha]);
    history.apply(&ObservedChangeSet {
        meta: EventMeta {
            id: EventId::new("observed:stale-ready"),
            ts: 1,
            actor: EventActor::System,
            correlation: None,
            causation: None,
            execution_context: None,
        },
        trigger: ChangeTrigger::ManualReindex,
        files: vec![FileId(1)],
        previous_path: Some("workspace/src/lib.rs".into()),
        current_path: Some("workspace/src/lib.rs".into()),
        added: Vec::new(),
        removed: Vec::new(),
        updated: vec![(
            ObservedNode {
                node: Node {
                    id: NodeId::new("demo", "demo::alpha", NodeKind::Function),
                    name: "alpha".into(),
                    kind: NodeKind::Function,
                    file: FileId(1),
                    span: Span::line(1),
                    language: Language::Rust,
                },
                fingerprint: prism_ir::SymbolFingerprint::with_parts(1, Some(1), None, None),
            },
            ObservedNode {
                node: Node {
                    id: NodeId::new("demo", "demo::alpha", NodeKind::Function),
                    name: "alpha".into(),
                    kind: NodeKind::Function,
                    file: FileId(1),
                    span: Span::line(1),
                    language: Language::Rust,
                },
                fingerprint: prism_ir::SymbolFingerprint::with_parts(1, Some(1), None, None),
            },
        )],
        edge_added: Vec::new(),
        edge_removed: Vec::new(),
    });

    let outcomes = OutcomeMemory::new();
    let coordination = CoordinationStore::new();
    let (plan_id, _) = coordination
        .create_plan(
            EventMeta {
                id: EventId::new("coord:plan:stale-ready"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            PlanCreateInput {
                title: "Keep published readiness aligned".into(),
                goal: "Keep published readiness aligned".into(),
                status: None,
                policy: Some(CoordinationPolicy {
                    stale_after_graph_change: true,
                    ..CoordinationPolicy::default()
                }),
                spec_refs: Vec::new(),
            },
        )
        .unwrap();
    coordination
        .create_task(
            EventMeta {
                id: EventId::new("coord:task:stale-ready"),
                ts: 2,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Unbound task".into(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                anchors: Vec::new(),
                depends_on: Vec::new(),
                coordination_depends_on: Vec::new(),
                integrated_depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: WorkspaceRevision::default(),
                spec_refs: Vec::new(),
            },
        )
        .unwrap();

    let prism = Prism::with_history_outcomes_coordination_and_projections(
        graph,
        history,
        outcomes,
        coordination.snapshot(),
        ProjectionIndex::default(),
    );

    assert_eq!(prism.ready_tasks(&plan_id, 10).len(), 1);

    let summary = prism
        .plan_summary(&plan_id)
        .expect("plan summary should exist");
    assert_eq!(summary.total_nodes, 1);
    assert_eq!(summary.actionable_nodes, 1);
    assert_eq!(summary.execution_blocked_nodes, 0);
    assert_eq!(summary.stale_nodes, 0);
}

#[test]
fn plans_cache_invalidates_when_workspace_revision_changes() {
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
    let coordination = CoordinationStore::new();
    let (plan_id, _) = coordination
        .create_plan(
            EventMeta {
                id: EventId::new("coord:plan:plans-cache-invalidation"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            PlanCreateInput {
                title: "Invalidate cached plan summaries on workspace revision changes".into(),
                goal: "Invalidate cached plan summaries on workspace revision changes".into(),
                status: None,
                policy: Some(CoordinationPolicy {
                    stale_after_graph_change: true,
                    ..CoordinationPolicy::default()
                }),
                spec_refs: Vec::new(),
            },
        )
        .unwrap();
    coordination
        .create_task(
            EventMeta {
                id: EventId::new("coord:task:plans-cache-invalidation"),
                ts: 2,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Track alpha".into(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                anchors: vec![AnchorRef::Node(alpha)],
                depends_on: Vec::new(),
                coordination_depends_on: Vec::new(),
                integrated_depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: WorkspaceRevision {
                    graph_version: 0,
                    git_commit: None,
                },
                spec_refs: Vec::new(),
            },
        )
        .unwrap();

    let prism = Prism::with_history_outcomes_coordination_and_projections(
        graph,
        history,
        outcomes,
        coordination.snapshot(),
        ProjectionIndex::default(),
    );
    prism.set_workspace_revision(WorkspaceRevision {
        graph_version: 0,
        git_commit: None,
    });

    let initial = prism
        .plans(None, None, None)
        .into_iter()
        .find(|entry| entry.plan_id == plan_id)
        .expect("plan should be listed before workspace drift");
    assert_eq!(initial.plan_summary.actionable_nodes, 1);
    assert_eq!(initial.plan_summary.stale_nodes, 0);

    prism.set_workspace_revision(WorkspaceRevision {
        graph_version: 1,
        git_commit: None,
    });

    let updated = prism
        .plans(None, None, None)
        .into_iter()
        .find(|entry| entry.plan_id == plan_id)
        .expect("plan should still be listed after workspace drift");
    assert_eq!(updated.plan_summary.actionable_nodes, 0);
    assert_eq!(updated.plan_summary.execution_blocked_nodes, 1);
    assert_eq!(updated.plan_summary.stale_nodes, 1);
}

#[test]
fn persisted_coordination_snapshot_updates_task_backed_plan_nodes() {
    let graph = Graph::new();
    let history = HistoryStore::new();
    let outcomes = OutcomeMemory::new();
    let coordination = CoordinationStore::new();
    let (plan_id, _) = coordination
        .create_plan(
            EventMeta {
                id: EventId::new("coord:plan:persist-task-status"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            PlanCreateInput {
                title: "Persist task-backed status".into(),
                goal: "Keep task and plan runtime in sync".into(),
                status: None,
                policy: Some(CoordinationPolicy::default()),
                spec_refs: Vec::new(),
            },
        )
        .unwrap();
    let (task_id, _) = coordination
        .create_task(
            EventMeta {
                id: EventId::new("coord:task:persist-task-status"),
                ts: 2,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Finish shared coordination sync".into(),
                status: Some(prism_ir::CoordinationTaskStatus::InProgress),
                assignee: None,
                session: Some(SessionId::new("session:persist-task-status")),
                worktree_id: None,
                branch_ref: None,
                anchors: Vec::new(),
                depends_on: Vec::new(),
                coordination_depends_on: Vec::new(),
                integrated_depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: WorkspaceRevision::default(),
                spec_refs: Vec::new(),
            },
        )
        .unwrap();

    let prism = Prism::with_history_outcomes_coordination_and_projections(
        graph,
        history,
        outcomes,
        coordination.snapshot(),
        ProjectionIndex::default(),
    );
    assert_eq!(
        prism
            .coordination_task_v2(&TaskId::new(task_id.0.clone()))
            .expect("task-backed coordination view")
            .status,
        EffectiveTaskStatus::Active
    );

    coordination
        .update_task(
            EventMeta {
                id: EventId::new("coord:task:persist-task-status:update"),
                ts: 3,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            TaskUpdateInput {
                task_id: task_id.clone(),
                kind: None,
                status: Some(prism_ir::CoordinationTaskStatus::Completed),
                published_task_status: None,
                git_execution: None,
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                title: None,
                summary: None,
                anchors: None,
                bindings: None,
                depends_on: None,
                coordination_depends_on: None,
                integrated_depends_on: None,
                acceptance: None,
                validation_refs: None,
                is_abstract: None,
                base_revision: Some(WorkspaceRevision::default()),
                priority: None,
                tags: None,
                completion_context: Some(TaskCompletionContext::default()),
                spec_refs: None,
            },
            WorkspaceRevision::default(),
            3,
        )
        .unwrap();

    prism
        .persist_coordination_snapshot(coordination.snapshot())
        .expect("persisted coordination snapshot should refresh plan runtime");

    assert_eq!(
        prism
            .coordination_task(&task_id)
            .expect("coordination task")
            .status,
        prism_ir::CoordinationTaskStatus::Completed
    );
    assert_eq!(
        prism
            .coordination_task_v2(&TaskId::new(task_id.0.clone()))
            .expect("task-backed coordination view")
            .status,
        EffectiveTaskStatus::Completed
    );
    let summary = prism.plan_summary(&plan_id).expect("plan summary");
    assert_eq!(summary.completed_nodes, 1);
    assert_eq!(summary.in_progress_nodes, 0);
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
                execution_context: None,
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
fn contract_target_nodes_preserve_node_anchor_without_live_graph() {
    let target = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    let prism = Prism::new(Graph::new());

    let nodes = prism.contract_target_nodes(
        &ContractTarget {
            anchors: vec![AnchorRef::Node(target.clone())],
            concept_handles: Vec::new(),
        },
        8,
    );

    assert_eq!(nodes, vec![target]);
}

#[test]
fn contract_target_matching_skips_graph_enrichment_without_cognition() {
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

    let contract = ContractPacket {
        handle: "contract://alpha_file".into(),
        name: "alpha file".into(),
        summary: "File-backed contract target.".into(),
        aliases: Vec::new(),
        kind: ContractKind::Interface,
        subject: ContractTarget {
            anchors: vec![AnchorRef::File(FileId(1))],
            concept_handles: Vec::new(),
        },
        guarantees: Vec::new(),
        assumptions: Vec::new(),
        consumers: Vec::new(),
        validations: Vec::new(),
        stability: Default::default(),
        compatibility: Default::default(),
        evidence: Vec::new(),
        status: ContractStatus::Active,
        scope: ContractScope::Session,
        provenance: Default::default(),
        publication: None,
    };

    let prism = Prism::with_history_outcomes_coordination_and_projections(
        graph,
        HistoryStore::new(),
        OutcomeMemory::new(),
        CoordinationSnapshot::default(),
        ProjectionIndex::default(),
    );
    prism.replace_curated_contracts(vec![contract.clone()]);

    assert!(prism.contract_subject_matches_target(&alpha, &contract));

    prism.set_runtime_capabilities(PrismRuntimeMode::KnowledgeStorage.capabilities());

    assert!(!prism.contract_subject_matches_target(&alpha, &contract));
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
                execution_context: None,
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
                execution_context: None,
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
    let alpha_consumer_one = NodeId::new("demo", "demo::alpha_consumer_one", NodeKind::Function);
    let alpha_consumer_two = NodeId::new("demo", "demo::alpha_consumer_two", NodeKind::Function);
    graph.add_node(Node {
        id: alpha.clone(),
        name: "alpha".into(),
        kind: NodeKind::Function,
        file: FileId(1),
        span: Span::line(1),
        language: Language::Rust,
    });
    graph.add_node(Node {
        id: alpha_consumer_one.clone(),
        name: "alpha_consumer_one".into(),
        kind: NodeKind::Function,
        file: FileId(1),
        span: Span::line(3),
        language: Language::Rust,
    });
    graph.add_node(Node {
        id: alpha_consumer_two.clone(),
        name: "alpha_consumer_two".into(),
        kind: NodeKind::Function,
        file: FileId(1),
        span: Span::line(5),
        language: Language::Rust,
    });

    let mut history = HistoryStore::new();
    history.seed_nodes([
        alpha.clone(),
        alpha_consumer_one.clone(),
        alpha_consumer_two.clone(),
    ]);

    let outcomes = OutcomeMemory::new();
    outcomes
        .store_event(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:risk"),
                ts: 4,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:risk")),
                causation: None,
                execution_context: None,
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
                execution_context: None,
            },
            PlanCreateInput {
                title: "Risky edit".into(),
                goal: "Risky edit".into(),
                status: None,
                policy: Some(CoordinationPolicy {
                    review_required_above_risk_score: Some(0.2),
                    require_validation_for_completion: true,
                    ..CoordinationPolicy::default()
                }),
                spec_refs: Vec::new(),
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
                execution_context: None,
            },
            TaskCreateInput {
                plan_id,
                title: "Edit alpha".into(),
                status: None,
                assignee: None,
                session: Some(SessionId::new("session:a")),
                worktree_id: None,
                branch_ref: None,
                anchors: vec![AnchorRef::Node(alpha.clone())],
                depends_on: Vec::new(),
                coordination_depends_on: Vec::new(),
                integrated_depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                spec_refs: Vec::new(),
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
                execution_context: None,
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
                current_revision: WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                required_validations: vec!["test:alpha_integration".into()],
                validated_checks: Vec::new(),
                risk_score: Some(0.7),
                worktree_id: None,
                branch_ref: None,
            },
        )
        .unwrap();

    let mut projections = ProjectionIndex::derive(&history.snapshot(), &outcomes.snapshot());
    projections.upsert_curated_contract(ContractPacket {
        handle: "contract://alpha_surface".into(),
        name: "alpha surface".into(),
        summary: "alpha remains callable for recorded consumers.".into(),
        aliases: vec!["alpha contract".into()],
        kind: ContractKind::Interface,
        subject: ContractTarget {
            anchors: vec![AnchorRef::Node(alpha.clone())],
            concept_handles: Vec::new(),
        },
        guarantees: vec![ContractGuarantee {
            id: "alpha-callable".into(),
            statement: "alpha stays callable for downstream consumers.".into(),
            scope: Some("runtime".into()),
            strength: None,
            evidence_refs: Vec::new(),
        }],
        assumptions: vec!["consumers still pass the expected arguments".into()],
        consumers: vec![
            ContractTarget {
                anchors: vec![AnchorRef::Node(alpha_consumer_one)],
                concept_handles: Vec::new(),
            },
            ContractTarget {
                anchors: vec![AnchorRef::Node(alpha_consumer_two)],
                concept_handles: Vec::new(),
            },
        ],
        validations: Vec::new(),
        stability: Default::default(),
        compatibility: ContractCompatibility {
            compatible: Vec::new(),
            additive: vec!["Adding optional parameters is additive.".into()],
            risky: vec!["Changing the return payload shape is risky.".into()],
            breaking: vec!["Removing alpha is breaking for consumers.".into()],
            migrating: Vec::new(),
        },
        evidence: vec!["Captured from coordination risk investigation.".into()],
        status: ContractStatus::Active,
        scope: ContractScope::Session,
        provenance: Default::default(),
        publication: None,
    });
    let prism = Prism::with_history_outcomes_coordination_and_projections(
        graph,
        history,
        outcomes,
        coordination.snapshot(),
        projections,
    );

    let task_risk = prism.task_risk(&task_id, 5).unwrap();
    assert!(task_risk.review_required);
    assert_eq!(task_risk.likely_validations, vec!["test:alpha_integration"]);
    assert_eq!(
        task_risk.missing_validations,
        vec!["test:alpha_integration"]
    );
    assert_eq!(task_risk.contracts.len(), 1);
    assert!(task_risk
        .contract_review_notes
        .iter()
        .any(|note| note.contains("review compatibility guidance")));
    assert!(task_risk
        .contract_review_notes
        .iter()
        .any(|note| note.contains("2 recorded consumers")));
    assert!(task_risk
        .contract_review_notes
        .iter()
        .any(|note| note.contains("health is stale")));

    let artifact_risk = prism.artifact_risk(&artifact_id, 5).unwrap();
    assert!(artifact_risk.review_required);
    assert_eq!(
        artifact_risk.missing_validations,
        vec!["test:alpha_integration"]
    );
    assert_eq!(artifact_risk.contracts.len(), 1);
    assert!(artifact_risk
        .contract_review_notes
        .iter()
        .any(|note| note.contains("review compatibility guidance")));

    let blockers = prism.blockers(&task_id, 5);
    assert!(blockers
        .iter()
        .any(|blocker| blocker.kind == prism_coordination::BlockerKind::RiskReviewRequired));
    assert!(blockers
        .iter()
        .any(|blocker| blocker.kind == prism_coordination::BlockerKind::ValidationRequired));

    prism
        .outcome_memory()
        .store_event(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:coordination-validation:test"),
                ts: 4,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new(task_id.0.clone())),
                causation: None,
                execution_context: None,
            },
            anchors: vec![AnchorRef::Node(alpha.clone())],
            kind: OutcomeKind::TestRan,
            result: OutcomeResult::Success,
            summary: "alpha integration passed".into(),
            evidence: vec![OutcomeEvidence::Test {
                name: "test:alpha_integration".into(),
                passed: true,
            }],
            metadata: serde_json::Value::Null,
        })
        .unwrap();

    let task_risk = prism.task_risk(&task_id, 5).unwrap();
    assert!(task_risk.missing_validations.is_empty());
    let blockers = prism.blockers(&task_id, 5);
    assert!(!blockers
        .iter()
        .any(|blocker| blocker.kind == prism_coordination::BlockerKind::ValidationRequired));
}

#[test]
fn coordination_only_artifact_risk_uses_artifact_fields_without_cognition() {
    let coordination = CoordinationStore::new();
    let (plan_id, _) = coordination
        .create_plan(
            EventMeta {
                id: EventId::new("coord:plan:coordination-only-artifact-risk"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            PlanCreateInput {
                title: "Coordination-only artifact risk".into(),
                goal: "Artifact risk should not require cognition".into(),
                status: None,
                policy: Some(CoordinationPolicy {
                    review_required_above_risk_score: Some(0.2),
                    ..CoordinationPolicy::default()
                }),
                spec_refs: Vec::new(),
            },
        )
        .unwrap();
    let (task_id, _) = coordination
        .create_task(
            EventMeta {
                id: EventId::new("coord:task:coordination-only-artifact-risk"),
                ts: 2,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            TaskCreateInput {
                plan_id,
                title: "Edit alpha".into(),
                status: None,
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                anchors: Vec::new(),
                depends_on: Vec::new(),
                coordination_depends_on: Vec::new(),
                integrated_depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                spec_refs: Vec::new(),
            },
        )
        .unwrap();
    let (artifact_id, _) = coordination
        .propose_artifact(
            EventMeta {
                id: EventId::new("coord:artifact:coordination-only-artifact-risk"),
                ts: 3,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            ArtifactProposeInput {
                task_id: task_id.clone(),
                anchors: Vec::new(),
                diff_ref: Some("patch:artifact-risk".into()),
                evidence: Vec::new(),
                base_revision: WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                current_revision: WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                required_validations: vec!["test:artifact-check".into()],
                validated_checks: Vec::new(),
                risk_score: Some(0.7),
                worktree_id: None,
                branch_ref: None,
            },
        )
        .unwrap();

    let prism = Prism::with_history_outcomes_coordination_and_projections(
        Graph::new(),
        HistoryStore::new(),
        OutcomeMemory::new(),
        coordination.snapshot(),
        ProjectionIndex::default(),
    );
    prism.set_runtime_capabilities(PrismRuntimeMode::CoordinationOnly.capabilities());

    let artifact_risk = prism
        .artifact_risk(&artifact_id, 5)
        .expect("coordination-only artifact risk should use artifact fields");
    assert_eq!(artifact_risk.task_id, task_id);
    assert_eq!(artifact_risk.risk_score, 0.7);
    assert!(artifact_risk.review_required);
    assert_eq!(
        artifact_risk.required_validations,
        vec!["test:artifact-check"]
    );
    assert_eq!(
        artifact_risk.missing_validations,
        vec!["test:artifact-check"]
    );
    assert!(artifact_risk.contracts.is_empty());
    assert!(artifact_risk.contract_review_notes.is_empty());
    assert!(artifact_risk.co_change_neighbors.is_empty());
    assert!(artifact_risk.risk_events.is_empty());
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
                execution_context: None,
            },
            PlanCreateInput {
                title: "Ship alpha".into(),
                goal: "Ship alpha".into(),
                status: None,
                policy: None,
                spec_refs: Vec::new(),
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
                execution_context: None,
            },
            TaskCreateInput {
                plan_id,
                title: "Update alpha".into(),
                status: None,
                assignee: None,
                session: Some(SessionId::new("session:intent")),
                worktree_id: None,
                branch_ref: None,
                anchors: vec![AnchorRef::Node(alpha.clone())],
                depends_on: Vec::new(),
                coordination_depends_on: Vec::new(),
                integrated_depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: WorkspaceRevision::default(),
                spec_refs: Vec::new(),
            },
        )
        .unwrap();

    let prism = Prism::with_history_outcomes_coordination_and_projections(
        graph,
        HistoryStore::new(),
        OutcomeMemory::new(),
        coordination.snapshot(),
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

#[test]
fn incremental_intent_refresh_matches_fresh_derivation_for_observed_changes() {
    let spec = Node {
        id: NodeId::new(
            "demo",
            "demo::document::docs::spec_md::contract",
            NodeKind::MarkdownHeading,
        ),
        name: "Contract".into(),
        kind: NodeKind::MarkdownHeading,
        file: FileId(1),
        span: Span::line(1),
        language: Language::Markdown,
    };
    let alpha = Node {
        id: NodeId::new("demo", "demo::alpha", NodeKind::Function),
        name: "alpha".into(),
        kind: NodeKind::Function,
        file: FileId(2),
        span: Span::line(1),
        language: Language::Rust,
    };
    let alpha_test = Node {
        id: NodeId::new("demo", "demo::alpha_test", NodeKind::Function),
        name: "alpha_test".into(),
        kind: NodeKind::Function,
        file: FileId(3),
        span: Span::line(1),
        language: Language::Rust,
    };

    let mut old_graph = Graph::new();
    old_graph.add_node(spec.clone());
    old_graph.add_node(alpha.clone());
    old_graph.add_edge(Edge {
        kind: EdgeKind::Specifies,
        source: spec.id.clone(),
        target: alpha.id.clone(),
        origin: prism_ir::EdgeOrigin::Static,
        confidence: 0.8,
    });

    let mut new_graph = old_graph.clone();
    new_graph.add_node(alpha_test.clone());
    let validation_edge = Edge {
        kind: EdgeKind::Validates,
        source: spec.id.clone(),
        target: alpha_test.id.clone(),
        origin: prism_ir::EdgeOrigin::Static,
        confidence: 0.8,
    };
    new_graph.add_edge(validation_edge.clone());

    let prism = Prism::new(old_graph);
    let updated = prism.updated_intent_for_observed_changes(
        &new_graph,
        &[ObservedChangeSet {
            meta: EventMeta {
                id: EventId::new("evt:intent-refresh"),
                ts: 1,
                actor: EventActor::System,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            trigger: ChangeTrigger::ManualReindex,
            files: vec![FileId(1), FileId(3)],
            previous_path: None,
            current_path: None,
            added: vec![ObservedNode {
                node: alpha_test,
                fingerprint: prism_ir::SymbolFingerprint::new(1),
            }],
            removed: Vec::new(),
            updated: Vec::new(),
            edge_added: vec![validation_edge],
            edge_removed: Vec::new(),
        }],
    );

    let fresh = Prism::new(new_graph);
    assert_eq!(updated, fresh.intent_snapshot());
}

#[test]
fn policy_violations_expose_rejected_coordination_mutations() {
    let coordination = CoordinationStore::new();
    let (plan_id, _) = coordination
        .create_plan(
            EventMeta {
                id: EventId::new("coord:plan:audit"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            PlanCreateInput {
                title: "Require review".into(),
                goal: "Require review".into(),
                status: None,
                policy: Some(CoordinationPolicy {
                    require_review_for_completion: true,
                    ..CoordinationPolicy::default()
                }),
                spec_refs: Vec::new(),
            },
        )
        .unwrap();
    let (task_id, _) = coordination
        .create_task(
            EventMeta {
                id: EventId::new("coord:task:audit"),
                ts: 2,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Edit alpha".into(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: Some(SessionId::new("session:audit")),
                worktree_id: None,
                branch_ref: None,
                anchors: Vec::new(),
                depends_on: Vec::new(),
                coordination_depends_on: Vec::new(),
                integrated_depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                spec_refs: Vec::new(),
            },
        )
        .unwrap();
    coordination
        .update_task(
            EventMeta {
                id: EventId::new("coord:reject:audit"),
                ts: 3,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            TaskUpdateInput {
                task_id: task_id.clone(),
                kind: None,
                status: Some(prism_ir::CoordinationTaskStatus::Completed),
                published_task_status: None,
                git_execution: None,
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                title: None,
                summary: None,
                anchors: None,
                bindings: None,
                depends_on: None,
                coordination_depends_on: None,
                integrated_depends_on: None,
                acceptance: None,
                validation_refs: None,
                is_abstract: None,
                base_revision: None,
                priority: None,
                tags: None,
                completion_context: Some(TaskCompletionContext::default()),
                spec_refs: None,
            },
            WorkspaceRevision {
                graph_version: 1,
                git_commit: None,
            },
            3,
        )
        .unwrap_err();

    let prism = Prism::with_history_outcomes_coordination_and_projections(
        Graph::new(),
        HistoryStore::new(),
        OutcomeMemory::new(),
        coordination.snapshot(),
        ProjectionIndex::default(),
    );
    let violations = prism.policy_violations(Some(&plan_id), Some(&task_id), 10);
    assert_eq!(violations.len(), 1);
    assert!(
        violations[0]
            .violations
            .iter()
            .any(|violation| violation.code
                == prism_coordination::PolicyViolationCode::ReviewRequired)
    );
}

#[test]
fn coordination_transaction_rejects_empty_transaction_with_stable_reason() {
    let prism = Prism::new(Graph::new());
    let error = prism
        .execute_coordination_transaction(
            EventMeta {
                id: EventId::new("coord:tx:empty"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            CoordinationTransactionInput::default(),
        )
        .expect_err("empty transaction should reject before domain mutation");

    let CoordinationTransactionError::Rejected(rejection) = error else {
        panic!("expected rejected transaction");
    };
    assert_eq!(
        rejection.stage,
        CoordinationTransactionValidationStage::InputShape
    );
    assert_eq!(
        rejection.category,
        CoordinationTransactionRejectionCategory::InvalidInput
    );
    assert_eq!(rejection.reason_code, "empty_transaction");
}

#[test]
fn coordination_transaction_rejects_forward_task_client_refs_before_domain_stage() {
    let prism = Prism::new(Graph::new());
    let error = prism
        .execute_coordination_transaction(
            EventMeta {
                id: EventId::new("coord:tx:forward-task-client-ref"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            CoordinationTransactionInput {
                mutations: vec![
                    CoordinationTransactionMutation::PlanCreate {
                        client_plan_id: Some("plan".to_string()),
                        title: "Plan".to_string(),
                        goal: "Create tasks".to_string(),
                        status: None,
                        policy: None,
                        scheduling: None,
                        spec_refs: Vec::new(),
                    },
                    CoordinationTransactionMutation::TaskCreate {
                        client_task_id: Some("first".to_string()),
                        plan: CoordinationTransactionPlanRef::ClientId("plan".to_string()),
                        title: "First".to_string(),
                        status: None,
                        assignee: None,
                        session: None,
                        worktree_id: None,
                        branch_ref: None,
                        anchors: Vec::new(),
                        depends_on: vec![super::CoordinationTransactionTaskRef::ClientId(
                            "later".to_string(),
                        )],
                        coordination_depends_on: Vec::new(),
                        integrated_depends_on: Vec::new(),
                        acceptance: Vec::new(),
                        base_revision: WorkspaceRevision::default(),
                        spec_refs: Vec::new(),
                    },
                    CoordinationTransactionMutation::TaskCreate {
                        client_task_id: Some("later".to_string()),
                        plan: CoordinationTransactionPlanRef::ClientId("plan".to_string()),
                        title: "Later".to_string(),
                        status: None,
                        assignee: None,
                        session: None,
                        worktree_id: None,
                        branch_ref: None,
                        anchors: Vec::new(),
                        depends_on: Vec::new(),
                        coordination_depends_on: Vec::new(),
                        integrated_depends_on: Vec::new(),
                        acceptance: Vec::new(),
                        base_revision: WorkspaceRevision::default(),
                        spec_refs: Vec::new(),
                    },
                ],
                ..CoordinationTransactionInput::default()
            },
        )
        .expect_err("forward client references should reject before domain mutation");

    let CoordinationTransactionError::Rejected(rejection) = error else {
        panic!("expected rejected transaction");
    };
    assert_eq!(
        rejection.stage,
        CoordinationTransactionValidationStage::ObjectIdentity
    );
    assert_eq!(
        rejection.category,
        CoordinationTransactionRejectionCategory::NotFound
    );
    assert_eq!(rejection.reason_code, "forward_task_client_reference");
}

#[test]
fn coordination_transaction_rejects_stale_revision_preconditions() {
    let prism = Prism::new(Graph::new());
    prism
        .execute_coordination_transaction(
            EventMeta {
                id: EventId::new("coord:tx:seed-plan-revision"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            CoordinationTransactionInput {
                mutations: vec![CoordinationTransactionMutation::PlanCreate {
                    client_plan_id: Some("plan".to_string()),
                    title: "Seed".to_string(),
                    goal: "Seed".to_string(),
                    status: None,
                    policy: None,
                    scheduling: None,
                    spec_refs: Vec::new(),
                }],
                ..CoordinationTransactionInput::default()
            },
        )
        .expect("seed transaction should commit");

    let error = prism
        .execute_coordination_transaction(
            EventMeta {
                id: EventId::new("coord:tx:stale-revision"),
                ts: 2,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            CoordinationTransactionInput {
                mutations: vec![CoordinationTransactionMutation::PlanCreate {
                    client_plan_id: Some("next".to_string()),
                    title: "Next".to_string(),
                    goal: "Next".to_string(),
                    status: None,
                    policy: None,
                    scheduling: None,
                    spec_refs: Vec::new(),
                }],
                optimistic_preconditions: Some(json!({
                    "expectedRevision": 0
                })),
                ..CoordinationTransactionInput::default()
            },
        )
        .expect_err("stale revision should reject as a conflict");

    let CoordinationTransactionError::Rejected(rejection) = error else {
        panic!("expected rejected transaction");
    };
    assert_eq!(
        rejection.stage,
        CoordinationTransactionValidationStage::Conflict
    );
    assert_eq!(
        rejection.category,
        CoordinationTransactionRejectionCategory::Conflict
    );
    assert_eq!(rejection.reason_code, "stale_revision");
}

#[test]
fn coordination_transaction_rejects_stale_event_count_preconditions() {
    let prism = Prism::new(Graph::new());
    prism
        .execute_coordination_transaction(
            EventMeta {
                id: EventId::new("coord:tx:seed-plan"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            CoordinationTransactionInput {
                mutations: vec![CoordinationTransactionMutation::PlanCreate {
                    client_plan_id: Some("plan".to_string()),
                    title: "Seed".to_string(),
                    goal: "Seed".to_string(),
                    status: None,
                    policy: None,
                    scheduling: None,
                    spec_refs: Vec::new(),
                }],
                ..CoordinationTransactionInput::default()
            },
        )
        .expect("seed transaction should commit");

    let error = prism
        .execute_coordination_transaction(
            EventMeta {
                id: EventId::new("coord:tx:stale-event-count"),
                ts: 2,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            CoordinationTransactionInput {
                mutations: vec![CoordinationTransactionMutation::PlanCreate {
                    client_plan_id: Some("next".to_string()),
                    title: "Next".to_string(),
                    goal: "Next".to_string(),
                    status: None,
                    policy: None,
                    scheduling: None,
                    spec_refs: Vec::new(),
                }],
                optimistic_preconditions: Some(json!({
                    "expectedEventCount": 0
                })),
                ..CoordinationTransactionInput::default()
            },
        )
        .expect_err("stale event count should reject as a conflict");

    let CoordinationTransactionError::Rejected(rejection) = error else {
        panic!("expected rejected transaction");
    };
    assert_eq!(
        rejection.stage,
        CoordinationTransactionValidationStage::Conflict
    );
    assert_eq!(
        rejection.category,
        CoordinationTransactionRejectionCategory::Conflict
    );
    assert_eq!(rejection.reason_code, "stale_event_count");
}
