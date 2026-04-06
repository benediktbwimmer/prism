use rmcp::transport::{IntoTransport, Transport};
use serde_json::{json, Value};

use super::*;
use crate::tests_support::{
    call_tool_request, first_tool_content_json, host_with_session_internal,
    host_with_shared_session_and_features, host_with_shared_session_internal, initialize_client,
    initialized_notification, mutation_credential_json, retry_on_runtime_sync_busy,
    shared_workspace_session, temp_workspace, test_session,
    workspace_session_with_owner_credential,
};
use prism_coordination::{
    CoordinationPolicy, CoordinationSnapshot, CoordinationTask, Plan, PlanScheduling,
    TaskGitExecution,
};
use prism_history::HistoryStore;
use prism_ir::{
    CoordinationTaskId, PlanId, PlanKind, PlanNodeKind, PlanScope, PlanStatus, WorkspaceRevision,
};
use prism_memory::OutcomeMemory;
use prism_projections::ProjectionIndex;
use prism_query::Prism;
use prism_store::Graph;

#[tokio::test]
async fn mcp_server_reports_review_queues_and_blockers_via_prism_query() {
    let root = temp_workspace();
    let (session, credential) = workspace_session_with_owner_credential(&root);
    let server = PrismMcpServer::with_session(session);
    let (server_transport, client_transport) = tokio::io::duplex(4096);
    let server_task = tokio::spawn(async move { server.serve(server_transport).await });
    let mut client = IntoTransport::<rmcp::RoleClient, _, _>::into_transport(client_transport);

    let _ = initialize_client(&mut client).await;
    client.send(initialized_notification()).await.unwrap();
    let running = server_task
        .await
        .expect("server join should succeed")
        .expect("server should initialize");

    client
        .send(call_tool_request(
            2,
            "prism_mutate",
            json!({
                "action": "declare_work",
                "credential": mutation_credential_json(&credential),
                "input": {
                    "title": "Review coordination blockers"
                }
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();
    let declared_work = first_tool_content_json(client.receive().await.unwrap());
    assert_eq!(declared_work["action"], "declare_work");

    client
        .send(call_tool_request(
            3,
            "prism_mutate",
            json!({
                "action": "coordination",
                "credential": mutation_credential_json(&credential),
                "input": {
                    "kind": "plan_create",
                    "payload": { "title": "Review-gated change", "goal": "Review-gated change",
                        "policy": { "requireReviewForCompletion": true }
                    }
                }
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();
    let plan = first_tool_content_json(client.receive().await.unwrap());
    let plan_id = plan["result"]["state"]["id"].as_str().unwrap().to_string();

    client
        .send(call_tool_request(
            4,
            "prism_mutate",
            json!({
                "action": "coordination",
                "credential": mutation_credential_json(&credential),
                "input": {
                    "kind": "task_create",
                    "payload": {
                        "planId": plan_id,
                        "title": "Patch main",
                        "anchors": [{
                            "type": "node",
                            "crateName": "demo",
                            "path": "demo::main",
                            "kind": "function"
                        }]
                    }
                }
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();
    let task = first_tool_content_json(client.receive().await.unwrap());
    let task_id = task["result"]["state"]["id"].as_str().unwrap().to_string();

    client
        .send(call_tool_request(
            5,
            "prism_mutate",
            json!({
                "action": "artifact",
                "credential": mutation_credential_json(&credential),
                "input": {
                    "action": "propose",
                    "payload": {
                        "taskId": task_id,
                        "diffRef": "patch:review-gated"
                    }
                }
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();
    let artifact = first_tool_content_json(client.receive().await.unwrap());
    assert!(artifact["result"]["artifactId"].as_str().is_some());

    client
        .send(call_tool_request(
            6,
            "prism_query",
            json!({
                "code": format!(
                    r#"
return {{
  blockers: prism.blockers("{task_id}"),
  pendingReviews: prism.pendingReviews("{plan_id}"),
}};
"#
                ),
                "language": "ts",
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();
    let envelope = first_tool_content_json(client.receive().await.unwrap());
    assert_eq!(
        envelope["result"]["blockers"][0]["kind"],
        Value::String("ReviewRequired".to_string())
    );
    assert_eq!(
        envelope["result"]["blockers"][0]["causes"][0]["source"],
        Value::String("plan_policy".to_string())
    );
    assert_eq!(
        envelope["result"]["blockers"][0]["causes"][1]["source"],
        Value::String("artifact_state".to_string())
    );
    assert_eq!(
        envelope["result"]["pendingReviews"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        envelope["diagnostics"][0]["code"],
        Value::String("task_blocked".to_string())
    );

    running.cancel().await.unwrap();
}

#[test]
fn coordination_resume_mutation_dispatches_through_authenticated_host() {
    let root = temp_workspace();
    let (workspace, credential) = workspace_session_with_owner_credential(&root);
    let authenticated = workspace
        .authenticate_principal_credential(
            &prism_ir::CredentialId::new(credential.credential_id.clone()),
            &credential.principal_token,
        )
        .expect("credential should authenticate");
    let host = host_with_session_internal(workspace);

    let trace = host.begin_mutation_run(test_session(&host).as_ref(), "coordination");
    let plan = host
        .store_coordination_traced_authenticated(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "title": "Resume stale task", "goal": "Resume stale task",
                    "status": "active"
                }),
                task_id: None,
            },
            &trace,
            Some(&authenticated),
        )
        .expect("plan create should succeed");
    let plan_id = plan.state["id"].as_str().unwrap().to_string();

    let trace = host.begin_mutation_run(test_session(&host).as_ref(), "coordination");
    let task = host
        .store_coordination_traced_authenticated(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskCreate,
                payload: json!({
                    "planId": plan_id,
                    "title": "Long-running edit",
                    "assignee": "agent-a"
                }),
                task_id: None,
            },
            &trace,
            Some(&authenticated),
        )
        .expect("task create should succeed");
    let task_id = task.state["id"].as_str().unwrap().to_string();

    let trace = host.begin_mutation_run(test_session(&host).as_ref(), "coordination");
    let error = host
        .store_coordination_traced_authenticated(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::Resume,
                payload: json!({
                    "taskId": task_id.clone()
                }),
                task_id: None,
            },
            &trace,
            Some(&authenticated),
        )
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("does not have a stale or expired lease to resume"));
}

#[test]
fn coordination_workflow_helpers_summarize_inbox_context_and_claim_preview() {
    let root = temp_workspace();
    let workspace = shared_workspace_session(&root);
    let writer =
        host_with_shared_session_and_features(Arc::clone(&workspace), PrismMcpFeatures::full());
    let host = host_with_shared_session_and_features(workspace, PrismMcpFeatures::full());

    let plan = retry_on_runtime_sync_busy(|| {
        writer.store_coordination(
            test_session(&writer).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "title": "Coordinate alpha", "goal": "Coordinate alpha",
                    "policy": {
                        "requireReviewForCompletion": true,
                        "maxParallelEditorsPerAnchor": 1
                    }
                }),
                task_id: None,
            },
        )
    })
    .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();

    let task = retry_on_runtime_sync_busy(|| {
        writer.store_coordination(
            test_session(&writer).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Edit alpha",
                    "status": "Ready",
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::alpha",
                        "kind": "function"
                    }]
                }),
                task_id: None,
            },
        )
    })
    .unwrap();
    let task_id = task.state["id"].as_str().unwrap().to_string();

    retry_on_runtime_sync_busy(|| {
        writer.store_claim(
            test_session(&writer).as_ref(),
            PrismClaimArgs {
                action: ClaimActionInput::Acquire,
                payload: json!({
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::alpha",
                        "kind": "function"
                    }],
                    "capability": "Edit",
                    "mode": "SoftExclusive",
                    "coordinationTaskId": task_id.clone()
                }),
                task_id: None,
            },
        )
    })
    .unwrap();

    retry_on_runtime_sync_busy(|| {
        writer.store_artifact(
            test_session(&writer).as_ref(),
            PrismArtifactArgs {
                action: ArtifactActionInput::Propose,
                payload: json!({
                    "taskId": task.state["id"].as_str().unwrap(),
                    "diffRef": "patch:alpha"
                }),
                task_id: None,
            },
        )
    })
    .unwrap();

    let result = (0..40)
        .find_map(|attempt| {
            let state = host
                .execute(
                    test_session(&host),
                    &format!(
                        r#"
const alpha = prism.symbol("alpha");
return {{
  inbox: prism.coordinationInbox("{plan_id}"),
  context: prism.taskContext("{task_id}"),
  preview: prism.claimPreview({{
    anchors: alpha ? [alpha] : [],
    capability: "Edit",
    mode: "SoftExclusive",
  }}),
}};
"#
                    ),
                    QueryLanguage::Ts,
                )
                .unwrap();
            if state.result["inbox"]["pendingReviews"]
                .as_array()
                .is_some_and(|reviews| reviews.len() == 1)
            {
                Some(state)
            } else if attempt == 39 {
                Some(state)
            } else {
                std::thread::sleep(std::time::Duration::from_millis(100));
                None
            }
        })
        .expect("coordination inbox result");

    let ready_tasks = result.result["inbox"]["readyTasks"]
        .as_array()
        .expect("readyTasks should be an array");
    assert!(ready_tasks.len() <= 1);
    if let Some(task) = ready_tasks.first() {
        assert_eq!(task["id"], Value::String(task_id.clone()));
    }
    assert_eq!(result.result["inbox"]["plan"]["id"], plan_id);
    assert_eq!(result.result["inbox"]["planV2"]["id"], plan_id);
    assert_eq!(result.result["inbox"]["planGraph"]["id"], plan_id);
    assert_eq!(result.result["inbox"]["children"]["planId"], plan_id);
    assert!(result.result["inbox"]["graphActionableTasks"]
        .as_array()
        .is_some_and(|tasks| tasks
            .iter()
            .any(|task| task["id"] == Value::String(task_id.clone()))));
    assert!(result.result["inbox"]["actionableTasks"]
        .as_array()
        .is_some_and(|tasks| tasks
            .iter()
            .any(|task| task["id"] == Value::String(task_id.clone()))));
    assert_eq!(
        result.result["inbox"]["planExecution"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        result.result["inbox"]["pendingReviews"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(result.result["inbox"]["planSummary"]["planId"], plan_id);
    assert_eq!(
        result.result["inbox"]["planNext"][0]["node"]["id"],
        Value::String(task_id.clone())
    );
    assert_eq!(result.result["context"]["task"]["id"], task_id);
    assert_eq!(result.result["context"]["taskNode"]["id"], task_id);
    assert!(result.result["context"]["taskExecution"].is_null());
    assert_eq!(result.result["context"]["planGraph"]["id"], plan_id);
    assert_eq!(result.result["context"]["planSummary"]["planId"], plan_id);
    assert_eq!(
        result.result["context"]["planNext"][0]["node"]["id"],
        Value::String(task_id.clone())
    );
    assert_eq!(result.result["context"]["taskV2"]["id"], task_id);
    assert!(result.result["context"]["dependencies"]
        .as_array()
        .is_some_and(|deps| deps.is_empty()));
    assert!(result.result["context"]["dependents"]
        .as_array()
        .is_some_and(|deps| deps.is_empty()));
    assert_eq!(
        result.result["context"]["claims"].as_array().unwrap().len(),
        1
    );
    assert_eq!(
        result.result["context"]["blockers"][0]["kind"],
        Value::String("ReviewRequired".to_string())
    );
    assert_eq!(result.result["preview"]["blocked"], Value::Bool(true));
    assert!(result.result["preview"]["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|conflict| conflict["severity"] == Value::String("Block".to_string())));
}

#[test]
fn multi_session_hosts_coordinate_handoff_review_and_neighbor_claims() {
    let root = temp_workspace();
    let workspace = shared_workspace_session(&root);
    let host_a = host_with_shared_session_internal(Arc::clone(&workspace));
    let host_b = host_with_shared_session_internal(workspace);
    if let Some(workspace) = host_a.workspace_session() {
        workspace.refresh_fs().unwrap();
        host_a.sync_workspace_revision(workspace).unwrap();
    }
    if let Some(workspace) = host_b.workspace_session() {
        workspace.refresh_fs().unwrap();
        host_b.sync_workspace_revision(workspace).unwrap();
    }

    host_b
        .configure_session(
            test_session(&host_b).as_ref(),
            PrismConfigureSessionArgs {
                limits: None,
                current_task_id: None,
                coordination_task_id: None,
                current_task_description: None,
                current_task_tags: None,
                clear_current_task: None,
                current_agent: Some("agent-b".to_string()),
                clear_current_agent: None,
            },
        )
        .unwrap();

    let plan = retry_on_runtime_sync_busy(|| {
        host_a.store_coordination(
            test_session(&host_a).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "title": "Coordinate alpha across sessions", "goal": "Coordinate alpha across sessions",
                    "policy": {
                        "requireReviewForCompletion": true,
                        "maxParallelEditorsPerAnchor": 1
                    }
                }),
                task_id: None,
            },
        )
    })
    .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();

    let task = retry_on_runtime_sync_busy(|| {
        host_a.store_coordination(
            test_session(&host_a).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Edit alpha",
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::alpha",
                        "kind": "function"
                    }]
                }),
                task_id: None,
            },
        )
    })
    .unwrap();
    let task_id = task.state["id"].as_str().unwrap().to_string();

    let first_claim = retry_on_runtime_sync_busy(|| {
        host_a.store_claim(
            test_session(&host_a).as_ref(),
            PrismClaimArgs {
                action: ClaimActionInput::Acquire,
                payload: json!({
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::alpha",
                        "kind": "function"
                    }],
                    "capability": "Edit",
                    "mode": "SoftExclusive",
                    "coordinationTaskId": task_id.clone()
                }),
                task_id: None,
            },
        )
    })
    .unwrap();
    assert!(first_claim.claim_id.is_some());

    let blocked_neighbor_claim = retry_on_runtime_sync_busy(|| {
        host_b.store_claim(
            test_session(&host_b).as_ref(),
            PrismClaimArgs {
                action: ClaimActionInput::Acquire,
                payload: json!({
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::beta",
                        "kind": "function"
                    }],
                    "capability": "Edit",
                    "mode": "SoftExclusive"
                }),
                task_id: None,
            },
        )
    })
    .unwrap();
    assert!(blocked_neighbor_claim.claim_id.is_none());
    assert!(blocked_neighbor_claim
        .conflicts
        .iter()
        .any(|conflict| conflict["severity"] == Value::String("Block".to_string())));
    assert!(blocked_neighbor_claim.conflicts.iter().any(|conflict| {
        conflict["overlapKinds"]
            .as_array()
            .map(|kinds: &Vec<Value>| kinds.iter().any(|kind| kind == "File"))
            .unwrap_or(false)
    }));

    retry_on_runtime_sync_busy(|| {
        host_a.store_coordination(
            test_session(&host_a).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::Handoff,
                payload: json!({
                    "taskId": task_id.clone(),
                    "toAgent": "agent-b",
                    "summary": "handoff alpha implementation to agent-b"
                }),
                task_id: None,
            },
        )
    })
    .unwrap();

    let handed_off = (0..10)
        .find_map(|attempt| {
            let state = host_b
                .execute(
                    test_session(&host_b),
                    &format!(r#"return prism.task("{task_id}");"#),
                    QueryLanguage::Ts,
                )
                .unwrap();
            if state.result["status"] == "Blocked" {
                Some(state)
            } else if attempt == 9 {
                Some(state)
            } else {
                std::thread::sleep(std::time::Duration::from_millis(50));
                None
            }
        })
        .expect("handoff state");
    assert_eq!(handed_off.result["assignee"], Value::Null);
    assert_eq!(handed_off.result["pendingHandoffTo"], "agent-b");
    assert_eq!(handed_off.result["status"], "Blocked");

    let blocked_update = retry_on_runtime_sync_busy(|| {
        host_b.store_coordination(
            test_session(&host_b).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::Update,
                payload: json!({
                    "id": task_id.clone(),
                    "status": "in-progress"
                }),
                task_id: None,
            },
        )
    })
    .unwrap();
    assert!(blocked_update.rejected);
    assert!(blocked_update
        .violations
        .iter()
        .any(|violation| violation.code == "handoff_pending"));
    if let Some(workspace) = host_b.workspace_session() {
        host_b.sync_workspace_revision(workspace).unwrap();
    }

    host_b
        .configure_session(
            test_session(&host_b).as_ref(),
            PrismConfigureSessionArgs {
                limits: None,
                current_task_id: None,
                coordination_task_id: None,
                current_task_description: None,
                current_task_tags: None,
                clear_current_task: None,
                current_agent: None,
                clear_current_agent: Some(true),
            },
        )
        .unwrap();
    let missing_agent = retry_on_runtime_sync_busy(|| {
        host_b.store_coordination(
            test_session(&host_b).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::HandoffAccept,
                payload: json!({
                    "taskId": task_id.clone(),
                    "agent": "agent-b"
                }),
                task_id: None,
            },
        )
    })
    .unwrap();
    assert!(missing_agent.rejected);
    assert!(missing_agent
        .violations
        .iter()
        .any(|violation| violation.code == "agent_identity_required"));
    if let Some(workspace) = host_b.workspace_session() {
        host_b.sync_workspace_revision(workspace).unwrap();
    }

    host_b
        .configure_session(
            test_session(&host_b).as_ref(),
            PrismConfigureSessionArgs {
                limits: None,
                current_task_id: None,
                coordination_task_id: None,
                current_task_description: None,
                current_task_tags: None,
                clear_current_task: None,
                current_agent: Some("agent-b".to_string()),
                clear_current_agent: None,
            },
        )
        .unwrap();

    let accepted = retry_on_runtime_sync_busy(|| {
        host_b.store_coordination(
            test_session(&host_b).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::HandoffAccept,
                payload: json!({
                    "taskId": task_id.clone(),
                    "agent": "agent-b"
                }),
                task_id: None,
            },
        )
    })
    .unwrap();
    assert_eq!(accepted.state["assignee"], "agent-b");
    assert_eq!(accepted.state["pendingHandoffTo"], Value::Null);
    assert_eq!(accepted.state["status"], "Ready");
    if let Some(workspace) = host_b.workspace_session() {
        host_b.sync_workspace_revision(workspace).unwrap();
    }

    let second_claim = retry_on_runtime_sync_busy(|| {
        host_b.store_claim(
            test_session(&host_b).as_ref(),
            PrismClaimArgs {
                action: ClaimActionInput::Acquire,
                payload: json!({
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::alpha",
                        "kind": "function"
                    }],
                    "capability": "Edit",
                    "mode": "SoftExclusive",
                    "coordinationTaskId": task_id.clone()
                }),
                task_id: None,
            },
        )
    })
    .unwrap();
    assert!(second_claim.claim_id.is_some());

    let artifact = retry_on_runtime_sync_busy(|| {
        host_b.store_artifact(
            test_session(&host_b).as_ref(),
            PrismArtifactArgs {
                action: ArtifactActionInput::Propose,
                payload: json!({
                    "taskId": task.state["id"].as_str().unwrap(),
                    "diffRef": "patch:alpha-shared"
                }),
                task_id: None,
            },
        )
    })
    .unwrap();
    let artifact_id = artifact.artifact_id.clone().unwrap();

    retry_on_runtime_sync_busy(|| {
        host_a.store_artifact(
            test_session(&host_a).as_ref(),
            PrismArtifactArgs {
                action: ArtifactActionInput::Review,
                payload: json!({
                    "artifactId": artifact_id,
                    "verdict": "approved",
                    "summary": "reviewed after handoff"
                }),
                task_id: None,
            },
        )
    })
    .unwrap();

    let reviewed_state = host_b
        .execute(
            test_session(&host_b),
            &format!(
                r#"
return {{
  artifacts: prism.artifacts("{task_id}"),
  pendingReviews: prism.pendingReviews("{plan_id}"),
}};
"#
            ),
            QueryLanguage::Ts,
        )
        .unwrap();
    assert_eq!(
        reviewed_state.result["artifacts"][0]["status"], "Approved",
        "reviewed artifact did not reload into host_b: {reviewed_state:#?}"
    );
    let resumed = retry_on_runtime_sync_busy(|| {
        host_b.store_coordination(
            test_session(&host_b).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::Update,
                payload: json!({
                    "id": task_id.clone(),
                    "status": "ready"
                }),
                task_id: None,
            },
        )
    })
    .unwrap();
    assert!(
        !resumed.rejected,
        "resume unexpectedly rejected after approval: {resumed:#?}"
    );
    assert_eq!(resumed.state["status"], "Ready");

    let completed = retry_on_runtime_sync_busy(|| {
        host_b.store_coordination(
            test_session(&host_b).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::Update,
                payload: json!({
                    "id": task_id.clone(),
                    "status": "completed"
                }),
                task_id: None,
            },
        )
    })
    .unwrap();
    assert!(
        !completed.rejected,
        "completion unexpectedly rejected: {completed:#?}"
    );
    assert_eq!(completed.state["status"], "Completed");

    let final_state = (0..120)
        .find_map(|attempt| {
            if attempt > 0 {
                host_a
                    .refresh_workspace()
                    .expect("host A refresh should succeed while waiting for completion");
                host_a
                    .workspace_session()
                    .expect("host A workspace session should exist")
                    .hydrate_coordination_runtime()
                    .expect("host A coordination runtime should hydrate while waiting");
            }
            let state = host_a
                .execute(
                    test_session(&host_a),
                    &format!(
                        r#"
return {{
  task: prism.task("{task_id}"),
  inbox: prism.coordinationInbox("{plan_id}"),
}};
"#
                    ),
                    QueryLanguage::Ts,
                )
                .unwrap();
            if state.result["task"]["status"] == "Completed" {
                Some(state)
            } else if attempt == 119 {
                Some(state)
            } else {
                std::thread::sleep(std::time::Duration::from_millis(100));
                None
            }
        })
        .expect("final coordination state");
    assert_eq!(final_state.result["task"]["status"], "Completed");
    assert_eq!(
        final_state.result["inbox"]["pendingReviews"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
}

#[tokio::test]
async fn mcp_server_exposes_canonical_v2_coordination_query_views() {
    let plan_id = PlanId::new("plan:canonical");
    let worktree_task_id = CoordinationTaskId::new("coord-task:worktree");
    let human_task_id = CoordinationTaskId::new("coord-task:human");
    let blocked_task_id = CoordinationTaskId::new("coord-task:blocked");
    let dependency_task_id = CoordinationTaskId::new("coord-task:dependency");
    let prism = Prism::with_history_outcomes_coordination_and_projections(
        Graph::new(),
        HistoryStore::new(),
        OutcomeMemory::new(),
        CoordinationSnapshot {
            plans: vec![Plan {
                id: plan_id.clone(),
                goal: "Ship canonical coordination".into(),
                title: "Ship canonical coordination".into(),
                status: PlanStatus::Active,
                policy: CoordinationPolicy::default(),
                scope: PlanScope::Repo,
                kind: PlanKind::TaskExecution,
                revision: 1,
                scheduling: PlanScheduling::default(),
                tags: Vec::new(),
                created_from: None,
                metadata: serde_json::Value::Null,
                authored_edges: Vec::new(),
                root_tasks: vec![
                    worktree_task_id.clone(),
                    human_task_id.clone(),
                    blocked_task_id.clone(),
                    dependency_task_id.clone(),
                ],
            }],
            tasks: vec![
                CoordinationTask {
                    id: worktree_task_id.clone(),
                    plan: plan_id.clone(),
                    kind: PlanNodeKind::Edit,
                    title: "Worktree task".into(),
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
                    metadata: serde_json::Value::Null,
                    git_execution: TaskGitExecution::default(),
                },
                CoordinationTask {
                    id: human_task_id.clone(),
                    plan: plan_id.clone(),
                    kind: PlanNodeKind::Edit,
                    title: "Human task".into(),
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
                    metadata: serde_json::json!({
                        "executor": {
                            "executorClass": "human"
                        }
                    }),
                    git_execution: TaskGitExecution::default(),
                },
                CoordinationTask {
                    id: blocked_task_id.clone(),
                    plan: plan_id.clone(),
                    kind: PlanNodeKind::Edit,
                    title: "Blocked task".into(),
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
                    depends_on: vec![dependency_task_id.clone()],
                    coordination_depends_on: Vec::new(),
                    integrated_depends_on: Vec::new(),
                    acceptance: Vec::new(),
                    validation_refs: Vec::new(),
                    is_abstract: false,
                    base_revision: WorkspaceRevision::default(),
                    priority: None,
                    tags: Vec::new(),
                    metadata: serde_json::Value::Null,
                    git_execution: TaskGitExecution::default(),
                },
                CoordinationTask {
                    id: dependency_task_id.clone(),
                    plan: plan_id.clone(),
                    kind: PlanNodeKind::Edit,
                    title: "Dependency".into(),
                    summary: None,
                    status: prism_ir::CoordinationTaskStatus::Completed,
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
                    metadata: serde_json::Value::Null,
                    git_execution: TaskGitExecution::default(),
                },
            ],
            claims: Vec::new(),
            artifacts: Vec::new(),
            reviews: Vec::new(),
            events: Vec::new(),
            next_plan: 1,
            next_task: 4,
            next_claim: 0,
            next_artifact: 0,
            next_review: 0,
        },
        ProjectionIndex::default(),
    );
    let server = PrismMcpServer::new(prism);
    let (server_transport, client_transport) = tokio::io::duplex(4096);
    let server_task = tokio::spawn(async move { server.serve(server_transport).await });
    let mut client = IntoTransport::<rmcp::RoleClient, _, _>::into_transport(client_transport);

    let _ = initialize_client(&mut client).await;
    client.send(initialized_notification()).await.unwrap();
    let running = server_task
        .await
        .expect("server join should succeed")
        .expect("server should initialize");

    client
        .send(call_tool_request(
            2,
            "prism_query",
            json!({
                "code": format!(
                    r#"
const blocked = prism.taskV2("{blocked_task_id}");
return {{
  compatPlan: prism.plan("{plan_id}"),
  plan: prism.planV2("{plan_id}"),
  compatTask: prism.task("{blocked_task_id}"),
  blocked,
  children: prism.children("{plan_id}"),
  dependencies: blocked ? prism.dependencies({{ kind: blocked.dependencies[0]?.kind, id: blocked.id }}) : [],
  dependents: blocked?.dependencies[0] ? prism.dependents(blocked.dependencies[0]) : [],
  graphActionableTasks: prism.graphActionableTasks(),
  actionableTasks: prism.actionableTasks("principal:owner"),
  portfolio: prism.portfolio(),
}};
"#,
                    blocked_task_id = blocked_task_id.0,
                    plan_id = plan_id.0,
                ),
                "language": "ts",
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();
    let envelope = first_tool_content_json(client.receive().await.unwrap());
    assert_eq!(envelope["result"]["compatPlan"]["id"], plan_id.0.as_str());
    assert_eq!(envelope["result"]["compatPlan"]["status"], "Active");
    assert_eq!(
        envelope["result"]["compatPlan"]["rootNodeIds"]
            .as_array()
            .expect("compat plan roots should be an array")
            .len(),
        4
    );
    assert_eq!(envelope["result"]["plan"]["id"], plan_id.0.as_str());
    assert_eq!(
        envelope["result"]["plan"]["children"]
            .as_array()
            .expect("plan children should be an array")
            .len(),
        4
    );
    assert_eq!(
        envelope["result"]["blocked"]["id"],
        blocked_task_id.0.as_str()
    );
    assert_eq!(envelope["result"]["blocked"]["status"], "pending");
    assert_eq!(
        envelope["result"]["compatTask"]["id"],
        blocked_task_id.0.as_str()
    );
    assert_eq!(
        envelope["result"]["compatTask"]["planId"],
        plan_id.0.as_str()
    );
    assert_eq!(envelope["result"]["compatTask"]["status"], "Ready");
    assert_eq!(
        envelope["result"]["compatTask"]["dependsOn"][0],
        dependency_task_id.0.as_str()
    );
    assert_eq!(
        envelope["result"]["blocked"]["dependencies"][0]["id"],
        dependency_task_id.0.as_str()
    );
    assert_eq!(envelope["result"]["children"]["planId"], plan_id.0.as_str());
    assert_eq!(
        envelope["result"]["children"]["children"]
            .as_array()
            .expect("children view should contain child refs")
            .len(),
        4
    );
    assert_eq!(
        envelope["result"]["dependencies"][0]["id"],
        dependency_task_id.0.as_str()
    );
    assert_eq!(
        envelope["result"]["dependents"][0]["id"],
        blocked_task_id.0.as_str()
    );

    let graph_actionable_ids = envelope["result"]["graphActionableTasks"]
        .as_array()
        .expect("graph actionable tasks should be an array")
        .iter()
        .map(|task| {
            task["id"]
                .as_str()
                .expect("actionable task should expose id")
        })
        .collect::<Vec<_>>();
    assert_eq!(graph_actionable_ids.len(), 3);
    assert!(graph_actionable_ids.contains(&worktree_task_id.0.as_str()));
    assert!(graph_actionable_ids.contains(&human_task_id.0.as_str()));
    assert!(graph_actionable_ids.contains(&blocked_task_id.0.as_str()));

    let actionable_ids = envelope["result"]["actionableTasks"]
        .as_array()
        .expect("principal actionable tasks should be an array")
        .iter()
        .map(|task| {
            task["id"]
                .as_str()
                .expect("actionable task should expose id")
        })
        .collect::<Vec<_>>();
    assert_eq!(actionable_ids.len(), 2);
    assert!(actionable_ids.contains(&worktree_task_id.0.as_str()));
    assert!(actionable_ids.contains(&blocked_task_id.0.as_str()));
    assert!(!actionable_ids.contains(&human_task_id.0.as_str()));

    let portfolio = envelope["result"]["portfolio"]
        .as_array()
        .expect("portfolio should be an array");
    assert_eq!(portfolio.len(), 1);
    assert_eq!(portfolio[0]["id"], plan_id.0.as_str());

    running.cancel().await.unwrap();
}
