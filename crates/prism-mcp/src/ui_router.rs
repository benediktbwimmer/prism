use std::path::PathBuf;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Html;
use axum::routing::get;
use axum::Router;

use crate::ui_assets::{prism_ui_index_html, prism_ui_unbuilt_html};

#[derive(Clone)]
pub(crate) struct PrismUiState {
    pub(crate) root: PathBuf,
}

pub(crate) fn routes(state: PrismUiState) -> Router {
    Router::new()
        .route("/", get(prism_ui_index))
        .route("/dashboard", get(prism_ui_index))
        .route("/dashboard/", get(prism_ui_index))
        .route("/plans", get(prism_ui_index))
        .route("/plans/", get(prism_ui_index))
        .route("/graph", get(prism_ui_index))
        .route("/graph/", get(prism_ui_index))
        .with_state(state)
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use tower::util::ServiceExt;

    use crate::tests_support::temp_workspace;

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

        let router = routes(PrismUiState { root });

        for path in ["/", "/dashboard", "/plans", "/graph"] {
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
