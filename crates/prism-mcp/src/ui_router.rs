use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header::CONTENT_TYPE, Response, StatusCode};
use axum::response::Html;
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;

use crate::ui_assets::{prism_ui_asset, prism_ui_index_html, prism_ui_unbuilt_html};
use crate::ui_read_models::QueryHostUiReadModelsExt;
use crate::ui_types::{
    PrismGraphView, PrismOverviewView, PrismPlansView, PrismUiApiPlaceholderView,
    PrismUiSessionBootstrapView,
};
use crate::QueryHost;

#[derive(Clone)]
pub(crate) struct PrismUiState {
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
        .route("/api/v1/plans/{plan_id}/graph", get(prism_ui_plan_graph))
        .route("/api/v1/tasks/{task_id}", get(prism_ui_task_placeholder))
        .route("/api/v1/fleet", get(prism_ui_fleet_placeholder))
        .route("/dashboard", get(prism_ui_index))
        .route("/dashboard/", get(prism_ui_index))
        .route("/dashboard/favicon.svg", get(prism_ui_favicon))
        .route("/dashboard/assets/{*path}", get(prism_ui_dashboard_asset))
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
        .ui_plans_view(query.plan_id.as_deref())
        .map(Json)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}

async fn prism_ui_plan_graph(
    State(state): State<PrismUiState>,
    Path(plan_id): Path<String>,
) -> std::result::Result<Json<prism_js::PlanGraphView>, (StatusCode, String)> {
    let graph = state
        .host
        .ui_plan_graph_view(&plan_id)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    match graph {
        Some(graph) => Ok(Json(graph)),
        None => Err((StatusCode::NOT_FOUND, format!("plan graph not found: {plan_id}"))),
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

async fn prism_ui_task_placeholder(
    State(state): State<PrismUiState>,
    Path(task_id): Path<String>,
) -> Json<PrismUiApiPlaceholderView> {
    Json(state.host.ui_placeholder_view(
        &format!("/api/v1/tasks/{task_id}"),
        "Task detail read models land in the next operator-console backend task.",
    ))
}

async fn prism_ui_fleet_placeholder(
    State(state): State<PrismUiState>,
) -> Json<PrismUiApiPlaceholderView> {
    Json(state.host.ui_placeholder_view(
        "/api/v1/fleet",
        "Fleet timeline and runtime utilization read models land in a later operator-console backend task.",
    ))
}

async fn prism_ui_favicon(
    State(state): State<PrismUiState>,
) -> std::result::Result<Response<Body>, (StatusCode, String)> {
    prism_ui_asset_response(&state, "favicon.svg")
}

async fn prism_ui_dashboard_asset(
    State(state): State<PrismUiState>,
    Path(path): Path<String>,
) -> std::result::Result<Response<Body>, (StatusCode, String)> {
    prism_ui_asset_response(&state, &format!("assets/{path}"))
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
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use serde_json::Value;
    use tower::util::ServiceExt;

    use crate::tests_support::{host_with_session, temp_workspace};
    use crate::ui_assets::prism_ui_index_html;
    use prism_core::index_workspace_session;

    #[tokio::test]
    async fn ui_routes_share_the_same_shell_document() {
        let root = temp_workspace();
        let host = Arc::new(host_with_session(index_workspace_session(&root).unwrap()));
        let router = routes(PrismUiState { host, root });

        for path in ["/", "/plans", "/graph", "/fleet", "/dashboard"] {
            let response = router
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            assert!(std::str::from_utf8(&body)
                .unwrap()
                .contains("<title>"));
        }
    }

    #[tokio::test]
    async fn ui_routes_serve_bundled_assets() {
        let root = temp_workspace();
        let index = prism_ui_index_html(&root).unwrap().unwrap();
        let asset_path = index
            .split('"')
            .find(|segment| segment.starts_with("/dashboard/assets/"))
            .expect("embedded dashboard asset path")
            .to_string();
        let host = Arc::new(host_with_session(index_workspace_session(&root).unwrap()));
        let router = routes(PrismUiState { host, root });

        let favicon = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/dashboard/favicon.svg")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(favicon.status(), StatusCode::OK);

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
    }

    #[tokio::test]
    async fn ui_v1_session_bootstrap_is_available() {
        let root = temp_workspace();
        let host = Arc::new(host_with_session(index_workspace_session(&root).unwrap()));
        let router = routes(PrismUiState { host, root });

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
    async fn ui_v1_task_and_fleet_placeholders_are_stable_json() {
        let root = temp_workspace();
        let host = Arc::new(host_with_session(index_workspace_session(&root).unwrap()));
        let router = routes(PrismUiState { host, root });

        for path in ["/api/v1/tasks/demo-task", "/api/v1/fleet"] {
            let response = router
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            let value: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(value["status"], Value::from("not_implemented"));
        }
    }
}
