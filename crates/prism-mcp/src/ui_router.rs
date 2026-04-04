use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::Html;
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;

use crate::ui_assets::{prism_ui_index_html, prism_ui_unbuilt_html};
use crate::ui_read_models::QueryHostUiReadModelsExt;
use crate::ui_types::{PrismGraphView, PrismOverviewView, PrismPlansView};
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
        .route("/", get(prism_ui_index))
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use tower::util::ServiceExt;

    use crate::tests_support::{host_with_session, temp_workspace};
    use prism_core::index_workspace_session;

    #[tokio::test]
    async fn ui_routes_share_the_same_shell_document() {
        let root = temp_workspace();
        let dist = root.join("www").join("dashboard").join("dist");
        std::fs::create_dir_all(&dist).unwrap();
        std::fs::write(
            dist.join("index.html"),
            "<!doctype html><title>PRISM</title>",
        )
        .unwrap();

        let host = Arc::new(host_with_session(index_workspace_session(&root).unwrap()));
        let router = routes(PrismUiState { host, root });

        for path in ["/", "/plans", "/graph"] {
            let response = router
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            assert!(std::str::from_utf8(&body)
                .unwrap()
                .contains("<title>PRISM</title>"));
        }
    }
}
