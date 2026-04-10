use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{Response, StatusCode, header::CONTENT_TYPE};
use axum::response::Html;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::Value;

use crate::ui_assets::{
    prism_ui_asset, prism_ui_favicon_asset, prism_ui_index_html, prism_ui_unbuilt_html,
};
use crate::ui_mutations::{PrismUiMutateRequest, map_ui_mutation_error, resolve_ui_mutation_args};
use crate::ui_read_models::{QueryHostUiReadModelsExt, UiPlansQueryOptions};
use crate::ui_types::{
    PrismGraphView, PrismOverviewView, PrismPlanDetailView, PrismPlansView, PrismUiFleetView,
    PrismUiSessionBootstrapView, PrismUiTaskDetailView,
};
use crate::{PrismMcpServer, PrismMutationResult, QueryHost};

#[derive(Clone)]
pub(crate) struct PrismUiState {
    pub(crate) server: Arc<PrismMcpServer>,
    pub(crate) host: Arc<QueryHost>,
    pub(crate) root: PathBuf,
}

pub(crate) fn routes(state: PrismUiState) -> Router {
    Router::new()
        .route("/api/overview", get(prism_ui_overview))
        .route("/api/plans", get(prism_ui_plans))
        .route("/api/graph", get(prism_ui_graph))
        .route("/api/v1/session", get(prism_ui_session))
        .route("/api/v1/plans", get(prism_ui_plans))
        .route("/api/v1/plans/{plan_id}/detail", get(prism_ui_plan_detail))
        .route("/api/v1/tasks/{task_id}", get(prism_ui_task_detail))
        .route("/api/v1/fleet", get(prism_ui_fleet))
        .route("/api/v1/mutate", post(prism_ui_mutate))
        .route("/favicon.svg", get(prism_ui_favicon))
        .route("/assets/{*path}", get(prism_ui_root_asset))
        .route("/", get(prism_ui_index))
        .route("/fleet", get(prism_ui_index))
        .route("/fleet/", get(prism_ui_index))
        .route("/plans", get(prism_ui_index))
        .route("/plans/", get(prism_ui_index))
        .route("/graph", get(prism_ui_index))
        .route("/graph/", get(prism_ui_index))
        .with_state(state)
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlansQuery {
    plan_id: Option<String>,
    status: Option<String>,
    search: Option<String>,
    sort: Option<String>,
    agent: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphQuery {
    concept_handle: Option<String>,
}

async fn prism_ui_index(
    State(state): State<PrismUiState>,
) -> std::result::Result<Html<String>, (StatusCode, String)> {
    match prism_ui_index_html(&state.root) {
        Ok(Some(html)) => Ok(Html(html)),
        Ok(None) => Ok(Html(prism_ui_unbuilt_html(&state.root))),
        Err(error) => Err((StatusCode::INTERNAL_SERVER_ERROR, error.to_string())),
    }
}

async fn prism_ui_overview(
    State(state): State<PrismUiState>,
) -> std::result::Result<Json<PrismOverviewView>, (StatusCode, String)> {
    state
        .host
        .ui_overview_view()
        .map(Json)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}

async fn prism_ui_session(
    State(state): State<PrismUiState>,
) -> std::result::Result<Json<PrismUiSessionBootstrapView>, (StatusCode, String)> {
    state
        .host
        .ui_session_bootstrap_view()
        .map(Json)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}

async fn prism_ui_plans(
    State(state): State<PrismUiState>,
    Query(query): Query<PlansQuery>,
) -> std::result::Result<Json<PrismPlansView>, (StatusCode, String)> {
    state
        .host
        .ui_plans_view(UiPlansQueryOptions {
            selected_plan_id: query.plan_id,
            status: query.status,
            search: query.search,
            sort: query.sort,
            agent: query.agent,
        })
        .map(Json)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}

async fn prism_ui_plan_detail(
    State(state): State<PrismUiState>,
    Path(plan_id): Path<String>,
) -> std::result::Result<Json<PrismPlanDetailView>, (StatusCode, String)> {
    let detail = state
        .host
        .ui_plan_detail_view(&plan_id)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    match detail {
        Some(detail) => Ok(Json(detail)),
        None => Err((
            StatusCode::NOT_FOUND,
            format!("plan detail not found: {plan_id}"),
        )),
    }
}

async fn prism_ui_graph(
    State(state): State<PrismUiState>,
    Query(query): Query<GraphQuery>,
) -> std::result::Result<Json<PrismGraphView>, (StatusCode, String)> {
    state
        .host
        .ui_graph_view(query.concept_handle.as_deref())
        .map(Json)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}

async fn prism_ui_task_detail(
    State(state): State<PrismUiState>,
    Path(task_id): Path<String>,
) -> std::result::Result<Json<PrismUiTaskDetailView>, (StatusCode, String)> {
    let detail = state
        .host
        .ui_task_detail_view(&task_id)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    match detail {
        Some(detail) => Ok(Json(detail)),
        None => Err((
            StatusCode::NOT_FOUND,
            format!("task detail not found: {task_id}"),
        )),
    }
}

async fn prism_ui_fleet(
    State(state): State<PrismUiState>,
) -> std::result::Result<Json<PrismUiFleetView>, (StatusCode, String)> {
    state
        .host
        .ui_fleet_view()
        .map(Json)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}

async fn prism_ui_mutate(
    State(state): State<PrismUiState>,
    Json(request): Json<PrismUiMutateRequest>,
) -> std::result::Result<Json<PrismMutationResult>, (StatusCode, Json<Value>)> {
    let args = resolve_ui_mutation_args(&state.root, state.host.workspace_session_ref(), request)?;
    state
        .server
        .execute_prism_mutation_via_tool(args)
        .map(Json)
        .map_err(map_ui_mutation_error)
}

async fn prism_ui_favicon(
    State(state): State<PrismUiState>,
) -> std::result::Result<Response<Body>, (StatusCode, String)> {
    let (bytes, mime) = prism_ui_favicon_asset(&state.root)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, mime)
        .body(Body::from(bytes))
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}

async fn prism_ui_root_asset(
    State(state): State<PrismUiState>,
    Path(path): Path<String>,
) -> std::result::Result<Response<Body>, (StatusCode, String)> {
    prism_ui_asset_response(&state, &format!("assets/{path}"))
}

fn prism_ui_asset_response(
    state: &PrismUiState,
    path: &str,
) -> std::result::Result<Response<Body>, (StatusCode, String)> {
    let Some((bytes, mime)) = prism_ui_asset(&state.root, path)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    else {
        return Err((StatusCode::NOT_FOUND, format!("ui asset not found: {path}")));
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, mime)
        .body(Body::from(bytes))
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, to_bytes};
    use axum::http::{Request, StatusCode};
    use serde_json::Value;
    use serde_json::json;
    use tower::util::ServiceExt;

    use crate::tests_support::{
        credentials_test_lock, demo_node, host_with_node, temp_workspace, test_session,
        workspace_session_with_owner_credential,
    };
    use crate::{CoordinationMutationKindInput, PrismCoordinationArgs};
    use prism_core::{CredentialProfile, CredentialsFile, PrismPaths, index_workspace_session};

    fn ui_state_from_root(root: &std::path::Path) -> PrismUiState {
        let server = Arc::new(PrismMcpServer::with_session(
            index_workspace_session(root).unwrap(),
        ));
        let host = Arc::clone(&server.host);
        PrismUiState {
            server,
            host,
            root: root.to_path_buf(),
        }
    }

    #[tokio::test]
    async fn ui_routes_share_the_same_shell_document() {
        let _guard = credentials_test_lock();
        let root = temp_workspace();
        let router = routes(ui_state_from_root(&root));

        for path in ["/", "/plans", "/graph", "/fleet"] {
            let response = router
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            assert!(std::str::from_utf8(&body).unwrap().contains("<title>"));
        }
    }

    #[tokio::test]
    async fn ui_routes_serve_bundled_assets() {
        let _guard = credentials_test_lock();
        let root = temp_workspace();
        let router = routes(ui_state_from_root(&root));
        let shell = router
            .clone()
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(shell.status(), StatusCode::OK);
        let shell_body = to_bytes(shell.into_body(), usize::MAX).await.unwrap();
        let shell_text = std::str::from_utf8(&shell_body).unwrap();
        let asset_path = shell_text
            .split('"')
            .find(|segment| segment.starts_with("/assets/"))
            .map(str::to_string);

        let favicon = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/favicon.svg")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(favicon.status(), StatusCode::OK);

        if let Some(asset_path) = asset_path {
            let asset = router
                .oneshot(
                    Request::builder()
                        .uri(asset_path)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(asset.status(), StatusCode::OK);
        } else {
            assert!(shell_text.contains("frontend source exists"));
        }
    }

    #[tokio::test]
    async fn ui_v1_session_bootstrap_is_available() {
        let _guard = credentials_test_lock();
        let root = temp_workspace();
        let router = routes(ui_state_from_root(&root));

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/api/v1/session")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["pollingIntervalMs"], Value::from(2000));
        assert!(value.get("runtime").is_some());
        assert!(value.get("session").is_some());
    }

    #[tokio::test]
    async fn ui_v1_session_bootstrap_exposes_active_local_operator_identity() {
        let _guard = credentials_test_lock();
        let root = temp_workspace();
        let (session, credential) = workspace_session_with_owner_credential(&root);
        let credentials_path = PrismPaths::for_workspace_root(&root)
            .unwrap()
            .credentials_path()
            .unwrap();
        let mut credentials = CredentialsFile::load(&credentials_path).unwrap();
        credentials.upsert_profile(
            CredentialProfile {
                profile: "ui-owner".to_string(),
                authority_id: "local-daemon".to_string(),
                principal_id: credential.principal_id.clone(),
                credential_id: credential.credential_id.clone(),
                principal_token: credential.principal_token.clone(),
                encrypted_secret: None,
                principal_metadata: None,
                credential_metadata: None,
            },
            true,
        );
        credentials.save(&credentials_path).unwrap();
        let server = Arc::new(PrismMcpServer::with_session(session));
        let router = routes(PrismUiState {
            server: Arc::clone(&server),
            host: Arc::clone(&server.host),
            root: root.clone(),
        });

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/api/v1/session")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            value["session"]["bridgeIdentity"]["status"],
            Value::from("locked_local_profile")
        );
        assert_eq!(
            value["session"]["bridgeIdentity"]["profile"],
            Value::from("ui-owner")
        );
        assert_eq!(
            value["session"]["bridgeIdentity"]["principalId"],
            Value::from(credential.principal_id)
        );
        assert!(
            value["session"]["bridgeIdentity"]["credentialId"]
                .as_str()
                .is_some_and(|id| id.starts_with("credential:"))
        );
    }

    #[tokio::test]
    async fn ui_v1_task_detail_and_fleet_view_are_stable_json() {
        let _guard = credentials_test_lock();
        let root = temp_workspace();
        let host = Arc::new(host_with_node(demo_node()));
        let session = test_session(host.as_ref());
        let plan = host
            .store_coordination(
                session.as_ref(),
                PrismCoordinationArgs {
                    kind: CoordinationMutationKindInput::PlanCreate,
                    payload: json!({
                        "title": "Task detail plan",
                        "goal": "Inspect task detail"
                    }),
                    task_id: None,
                },
            )
            .unwrap();
        let plan_id = plan.state["id"].as_str().unwrap().to_string();
        let blocker = host
            .store_coordination(
                session.as_ref(),
                PrismCoordinationArgs {
                    kind: CoordinationMutationKindInput::TaskCreate,
                    payload: json!({
                        "planId": plan_id,
                        "title": "Upstream blocker",
                        "summary": "Must finish first"
                    }),
                    task_id: None,
                },
            )
            .unwrap();
        let blocker_id = blocker.state["id"].as_str().unwrap().to_string();
        let task = host
            .store_coordination(
                session.as_ref(),
                PrismCoordinationArgs {
                    kind: CoordinationMutationKindInput::TaskCreate,
                    payload: json!({
                        "planId": plan_id,
                        "title": "Primary task",
                        "summary": "Editable operator console target",
                        "dependsOn": [blocker_id]
                    }),
                    task_id: None,
                },
            )
            .unwrap();
        let task_id = task.state["id"].as_str().unwrap().to_string();
        let server = Arc::new(PrismMcpServer::with_session(
            index_workspace_session(&root).unwrap(),
        ));
        let router = routes(PrismUiState { server, host, root });

        let task_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/tasks/{task_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(task_response.status(), StatusCode::OK);
        let task_body = to_bytes(task_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let task_value: Value = serde_json::from_slice(&task_body).unwrap();
        assert_eq!(task_value["task"]["id"], Value::from(task_id.clone()));
        assert_eq!(task_value["editable"]["title"], Value::from("Primary task"));
        assert!(task_value["claimHistory"].is_array());
        assert!(
            task_value["blockers"]
                .as_array()
                .is_some_and(|items| !items.is_empty())
        );
        assert!(task_value["artifacts"].is_array());
        assert!(task_value["recentCommits"].is_array());

        let fleet_response = router
            .oneshot(
                Request::builder()
                    .uri("/api/v1/fleet")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(fleet_response.status(), StatusCode::OK);
        let fleet_body = to_bytes(fleet_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let fleet_value: Value = serde_json::from_slice(&fleet_body).unwrap();
        assert!(fleet_value["generatedAt"].is_number());
        assert!(fleet_value["windowStart"].is_number());
        assert!(fleet_value["windowEnd"].is_number());
        assert!(fleet_value["lanes"].is_array());
        assert!(fleet_value["bars"].is_array());
    }

    #[tokio::test]
    async fn ui_v1_plans_support_filters_and_plan_detail() {
        let _guard = credentials_test_lock();
        let root = temp_workspace();
        let host = Arc::new(host_with_node(demo_node()));
        let session = test_session(host.as_ref());

        let alpha = host
            .store_coordination(
                session.as_ref(),
                PrismCoordinationArgs {
                    kind: CoordinationMutationKindInput::PlanCreate,
                    payload: json!({
                        "title": "Alpha execution plan",
                        "goal": "Ship alpha",
                        "status": "active",
                        "scheduling": {
                            "importance": 90,
                            "urgency": 50,
                            "manualBoost": 20
                        }
                    }),
                    task_id: None,
                },
            )
            .unwrap();
        let alpha_plan_id = alpha.state["id"].as_str().unwrap().to_string();
        let alpha_task = host
            .store_coordination(
                session.as_ref(),
                PrismCoordinationArgs {
                    kind: CoordinationMutationKindInput::TaskCreate,
                    payload: json!({
                        "planId": alpha_plan_id,
                        "title": "Implement alpha graph",
                        "summary": "Primary strategic graph node"
                    }),
                    task_id: None,
                },
            )
            .unwrap();
        host.store_coordination(
            session.as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::Update,
                payload: json!({
                    "id": alpha_task.state["id"].as_str().unwrap(),
                    "assignee": "runtime-alpha",
                    "status": "in_progress"
                }),
                task_id: None,
            },
        )
        .unwrap();

        let beta = host
            .store_coordination(
                session.as_ref(),
                PrismCoordinationArgs {
                    kind: CoordinationMutationKindInput::PlanCreate,
                    payload: json!({
                        "title": "Beta completion plan",
                        "goal": "Wrap beta",
                        "scheduling": {
                            "importance": 10,
                            "urgency": 10,
                            "manualBoost": 0
                        }
                    }),
                    task_id: None,
                },
            )
            .unwrap();
        let beta_plan_id = beta.state["id"].as_str().unwrap().to_string();
        host.store_coordination(
            session.as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanUpdate,
                payload: json!({
                    "planId": beta_plan_id,
                    "status": "completed"
                }),
                task_id: None,
            },
        )
        .unwrap();

        let server = Arc::new(PrismMcpServer::with_session(
            index_workspace_session(&root).unwrap(),
        ));
        let router = routes(PrismUiState { server, host, root });

        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/plans?status=active&search=alpha&sort=priority&agent=runtime-alpha")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["filters"]["status"], Value::from("active"));
        assert_eq!(value["filters"]["sort"], Value::from("priority"));
        assert_eq!(value["stats"]["visiblePlans"], Value::from(1));
        assert_eq!(
            value["plans"][0]["title"],
            Value::from("Alpha execution plan")
        );

        let detail_response = router
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/plans/{alpha_plan_id}/detail"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(detail_response.status(), StatusCode::OK);
        let detail_body = to_bytes(detail_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let detail_value: Value = serde_json::from_slice(&detail_body).unwrap();
        assert_eq!(detail_value["plan"]["planId"], Value::from(alpha_plan_id));
        assert!(
            detail_value["childTasks"]
                .as_array()
                .is_some_and(|tasks| !tasks.is_empty())
        );
        assert!(detail_value["children"].as_array().is_some());
    }

    #[tokio::test]
    async fn ui_v1_mutate_uses_local_profile_and_logs_like_prism_mutate() {
        let _guard = credentials_test_lock();
        let root = temp_workspace();
        let (session, credential) = workspace_session_with_owner_credential(&root);
        let credentials_path = PrismPaths::for_workspace_root(&root)
            .unwrap()
            .credentials_path()
            .unwrap();
        let mut credentials = CredentialsFile::load(&credentials_path).unwrap();
        credentials.upsert_profile(
            CredentialProfile {
                profile: "ui-owner".to_string(),
                authority_id: "local-daemon".to_string(),
                principal_id: credential.principal_id.clone(),
                credential_id: credential.credential_id.clone(),
                principal_token: credential.principal_token.clone(),
                encrypted_secret: None,
                principal_metadata: None,
                credential_metadata: None,
            },
            true,
        );
        credentials.save(&credentials_path).unwrap();
        let server = Arc::new(PrismMcpServer::with_session(session));
        let host = Arc::clone(&server.host);
        let router = routes(PrismUiState {
            server: Arc::clone(&server),
            host,
            root: root.clone(),
        });

        let declare = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/mutate")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "action": "declare_work",
                            "input": {
                                "title": "Operator console mutate"
                            }
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(declare.status(), StatusCode::OK);
        let declare_body = to_bytes(declare.into_body(), usize::MAX).await.unwrap();
        let declare_json: Value = serde_json::from_slice(&declare_body).unwrap();
        assert_eq!(declare_json["action"], json!("declare_work"));

        let mutate = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/mutate")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "action": "coordination",
                            "input": {
                                "kind": "plan_create",
                                "payload": {
                                    "title": "UI plan",
                                    "goal": "Mutate via the operator console backend"
                                }
                            }
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let mutate_status = mutate.status();
        let mutate_body = to_bytes(mutate.into_body(), usize::MAX).await.unwrap();
        assert_eq!(mutate_status, StatusCode::OK);
        let mutate_json: Value = serde_json::from_slice(&mutate_body).unwrap();
        assert_eq!(mutate_json["action"], json!("coordination"));
        assert_eq!(mutate_json["result"]["rejected"], json!(false));

        let records = server.mcp_call_log_store().records();
        assert!(records.iter().any(|record| {
            record.entry.call_type == "tool"
                && record.entry.name == "prism_mutate"
                && record
                    .mutation_compat
                    .as_ref()
                    .is_some_and(|trace| trace.entry.action == "mutate.coordination")
        }));
    }
}
