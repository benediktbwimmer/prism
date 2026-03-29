use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{KeepAlive, Sse};
use axum::response::{Html, IntoResponse};
use axum::routing::{get, get_service};
use axum::{Json, Router};
use serde::Deserialize;
use tower_http::services::{ServeDir, ServeFile};

use crate::dashboard_types::{
    DashboardBootstrapView, DashboardCoordinationQueuesView, DashboardCoordinationSummaryView,
    DashboardOperationDetailView, DashboardOperationsView, DashboardSummaryView,
    DashboardTaskSnapshotView,
};
use crate::runtime_views::runtime_status;
use crate::{
    dashboard_assets::{
        dashboard_assets_dir, dashboard_dist_dir, dashboard_index_html, dashboard_unbuilt_html,
    },
    QueryHost,
};

#[derive(Clone)]
pub(crate) struct DashboardAppState {
    pub(crate) host: Arc<QueryHost>,
    pub(crate) root: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OperationsQuery {
    limit: Option<usize>,
}

pub(crate) fn routes(state: DashboardAppState) -> Router {
    let assets_dir = dashboard_assets_dir(&state.root);
    let favicon_path = dashboard_dist_dir(&state.root).join("favicon.svg");
    Router::new()
        .route("/dashboard", get(dashboard_index))
        .route_service(
            "/dashboard/favicon.svg",
            get_service(ServeFile::new(favicon_path)),
        )
        .route("/dashboard/api/bootstrap", get(dashboard_bootstrap))
        .route("/dashboard/api/summary", get(dashboard_summary))
        .route("/dashboard/api/task", get(dashboard_task))
        .route("/dashboard/api/coordination", get(dashboard_coordination))
        .route(
            "/dashboard/api/coordination/queues",
            get(dashboard_coordination_queues),
        )
        .route("/dashboard/api/runtime", get(dashboard_runtime))
        .route("/dashboard/api/operations", get(dashboard_operations))
        .route(
            "/dashboard/api/operations/{id}",
            get(dashboard_operation_detail),
        )
        .route("/dashboard/events", get(dashboard_events))
        .nest_service("/dashboard/assets", get_service(ServeDir::new(assets_dir)))
        .with_state(state)
}

async fn dashboard_index(
    State(state): State<DashboardAppState>,
) -> std::result::Result<Html<String>, (StatusCode, String)> {
    match dashboard_index_html(&state.root) {
        Ok(Some(html)) => Ok(Html(html)),
        Ok(None) => Ok(Html(dashboard_unbuilt_html(&state.root))),
        Err(error) => Err(internal_error(error)),
    }
}

async fn dashboard_bootstrap(
    State(state): State<DashboardAppState>,
    Query(query): Query<OperationsQuery>,
) -> std::result::Result<Json<DashboardBootstrapView>, (StatusCode, String)> {
    let summary = state
        .host
        .dashboard_summary_view()
        .map_err(internal_error)?;
    let operations = state.host.dashboard_operations_view(query.limit);
    let task = state
        .host
        .dashboard_task_snapshot(None)
        .map_err(internal_error)?;
    let coordination = state
        .host
        .dashboard_coordination_summary()
        .map_err(internal_error)?;
    let coordination_queues = state
        .host
        .dashboard_coordination_queues()
        .map_err(internal_error)?;
    Ok(Json(DashboardBootstrapView {
        summary,
        operations,
        task,
        coordination,
        coordination_queues,
    }))
}

async fn dashboard_summary(
    State(state): State<DashboardAppState>,
) -> std::result::Result<Json<DashboardSummaryView>, (StatusCode, String)> {
    Ok(Json(
        state
            .host
            .dashboard_summary_view()
            .map_err(internal_error)?,
    ))
}

async fn dashboard_task(
    State(state): State<DashboardAppState>,
) -> std::result::Result<Json<DashboardTaskSnapshotView>, (StatusCode, String)> {
    Ok(Json(
        state
            .host
            .dashboard_task_snapshot(None)
            .map_err(internal_error)?,
    ))
}

async fn dashboard_coordination(
    State(state): State<DashboardAppState>,
) -> std::result::Result<Json<DashboardCoordinationSummaryView>, (StatusCode, String)> {
    Ok(Json(
        state
            .host
            .dashboard_coordination_summary()
            .map_err(internal_error)?,
    ))
}

async fn dashboard_coordination_queues(
    State(state): State<DashboardAppState>,
) -> std::result::Result<Json<DashboardCoordinationQueuesView>, (StatusCode, String)> {
    Ok(Json(
        state
            .host
            .dashboard_coordination_queues()
            .map_err(internal_error)?,
    ))
}

async fn dashboard_runtime(
    State(state): State<DashboardAppState>,
) -> std::result::Result<Json<prism_js::RuntimeStatusView>, (StatusCode, String)> {
    Ok(Json(
        runtime_status(state.host.as_ref()).map_err(internal_error)?,
    ))
}

async fn dashboard_operations(
    State(state): State<DashboardAppState>,
    Query(query): Query<OperationsQuery>,
) -> std::result::Result<Json<DashboardOperationsView>, (StatusCode, String)> {
    Ok(Json(state.host.dashboard_operations_view(query.limit)))
}

async fn dashboard_operation_detail(
    State(state): State<DashboardAppState>,
    Path(id): Path<String>,
) -> std::result::Result<Json<DashboardOperationDetailView>, (StatusCode, String)> {
    state
        .host
        .dashboard_operation_detail(&id)
        .map(Json)
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                format!("unknown dashboard operation `{id}`"),
            )
        })
}

async fn dashboard_events(
    State(state): State<DashboardAppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let last_event_id = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    let stream = state.host.dashboard_state().sse_stream(last_event_id);
    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(std::time::Duration::from_secs(15))
            .text("keepalive"),
    )
}

fn internal_error(error: anyhow::Error) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}
