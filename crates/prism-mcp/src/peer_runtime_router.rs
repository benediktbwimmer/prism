use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use prism_core::WorkspaceSession;
use prism_ir::{CredentialCapability, CredentialId};
use prism_js::{RuntimeSharedCoordinationRefView, RuntimeSharedCoordinationRuntimeDescriptorView};
use serde::{Deserialize, Serialize};

use crate::runtime_views::runtime_status;
use crate::QueryHost;

#[derive(Clone)]
pub(crate) struct PeerRuntimeAppState {
    pub(crate) host: Arc<QueryHost>,
    pub(crate) root: PathBuf,
}

pub(crate) fn routes(state: PeerRuntimeAppState) -> Router {
    Router::new()
        .route(prism_core::PEER_RUNTIME_READ_PATH, post(peer_runtime_read))
        .with_state(state)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PeerRuntimeReadKind {
    RuntimeDiagnostics,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PeerRuntimeReadRequest {
    credential_id: String,
    principal_token: String,
    kind: PeerRuntimeReadKind,
    #[serde(default)]
    runtime_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PeerRuntimeReadResponse {
    kind: &'static str,
    served_at: u64,
    runtime_descriptor: RuntimeSharedCoordinationRuntimeDescriptorView,
    shared_coordination_ref: Option<RuntimeSharedCoordinationRefView>,
}

async fn peer_runtime_read(
    State(state): State<PeerRuntimeAppState>,
    Json(request): Json<PeerRuntimeReadRequest>,
) -> Result<Json<PeerRuntimeReadResponse>, (StatusCode, Json<serde_json::Value>)> {
    let _ = &state.root;
    match request.kind {
        PeerRuntimeReadKind::RuntimeDiagnostics => {
            let workspace = state
                .host
                .workspace_session()
                .ok_or_else(|| service_error("workspace-backed runtime session is unavailable"))?;
            authenticate_peer_read(workspace, &request)?;
            let status =
                runtime_status(&state.host).map_err(|error| service_error(error.to_string()))?;
            let instance_id = workspace
                .event_execution_context(None, None, None)
                .instance_id;
            let shared = status.shared_coordination_ref.clone();
            let descriptor = shared
                .as_ref()
                .and_then(|view| {
                    instance_id.as_deref().and_then(|instance_id| {
                        view.runtime_descriptors
                            .iter()
                            .find(|descriptor| descriptor.runtime_id == instance_id)
                            .cloned()
                    })
                })
                .ok_or_else(|| service_error("live runtime descriptor is unavailable"))?;
            if let Some(expected_runtime_id) = request.runtime_id.as_deref() {
                if descriptor.runtime_id != expected_runtime_id {
                    return Err((
                        StatusCode::NOT_FOUND,
                        Json(serde_json::json!({
                            "code": "peer_runtime_not_found",
                            "message": "requested runtime descriptor is not served by this daemon",
                            "runtimeId": expected_runtime_id,
                        })),
                    ));
                }
            }
            Ok(Json(PeerRuntimeReadResponse {
                kind: "runtime_diagnostics",
                served_at: descriptor.last_seen_at,
                runtime_descriptor: descriptor,
                shared_coordination_ref: shared,
            }))
        }
    }
}

fn authenticate_peer_read(
    workspace: &WorkspaceSession,
    request: &PeerRuntimeReadRequest,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let authenticated = workspace
        .authenticate_principal_credential(
            &CredentialId::new(request.credential_id.clone()),
            &request.principal_token,
        )
        .map_err(|error| {
            (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({
                    "code": "peer_runtime_auth_failed",
                    "message": error.to_string(),
                    "credentialId": request.credential_id,
                })),
            )
        })?;
    if !authenticated
        .credential
        .capabilities
        .contains(&CredentialCapability::All)
        && !authenticated
            .credential
            .capabilities
            .contains(&CredentialCapability::ReadPeerRuntime)
    {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "code": "peer_runtime_capability_denied",
                "message": "credential lacks read_peer_runtime capability",
                "credentialId": request.credential_id,
            })),
        ));
    }
    Ok(())
}

fn service_error(message: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::json!({
            "code": "peer_runtime_unavailable",
            "message": message.into(),
        })),
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::Arc;

    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use prism_core::{
        default_workspace_shared_runtime, hydrate_workspace_session_with_options,
        local_peer_runtime_endpoint, sync_live_runtime_descriptor, BootstrapOwnerInput,
        MintPrincipalRequest, PrismPaths, WorkspaceSessionOptions,
    };
    use prism_ir::{CredentialCapability, PrincipalKind};
    use serde_json::Value;
    use tower::util::ServiceExt;

    use crate::tests_support::temp_workspace;
    use crate::{PrismMcpFeatures, QueryHost};

    use super::{routes, PeerRuntimeAppState};

    fn init_git_workspace(branch: &str) -> PathBuf {
        let root = temp_workspace();
        Command::new("git")
            .args(["init", "-b", branch])
            .current_dir(&root)
            .output()
            .expect("git init should succeed");
        Command::new("git")
            .args(["config", "user.name", "PRISM Tests"])
            .current_dir(&root)
            .output()
            .expect("git config user.name should succeed");
        Command::new("git")
            .args(["config", "user.email", "tests@example.com"])
            .current_dir(&root)
            .output()
            .expect("git config user.email should succeed");
        root
    }

    #[tokio::test]
    async fn peer_runtime_read_requires_explicit_capability() {
        let root = init_git_workspace("task/peer-runtime-denied");
        let session = hydrate_workspace_session_with_options(
            &root,
            WorkspaceSessionOptions {
                coordination: true,
                shared_runtime: default_workspace_shared_runtime(&root).unwrap(),
                hydrate_persisted_projections: false,
                hydrate_persisted_co_change: false,
            },
        )
        .unwrap();
        let owner = session
            .bootstrap_owner_principal(BootstrapOwnerInput {
                authority_id: None,
                name: "Owner".to_string(),
                role: Some("owner".to_string()),
            })
            .unwrap();
        let uri_path = PrismPaths::for_workspace_root(&root)
            .unwrap()
            .mcp_http_uri_path()
            .unwrap();
        std::fs::create_dir_all(uri_path.parent().unwrap()).unwrap();
        std::fs::write(&uri_path, "http://127.0.0.1:52695/mcp").unwrap();
        sync_live_runtime_descriptor(&root).unwrap();

        let owner_auth = session
            .authenticate_principal_credential(
                &owner.credential.credential_id,
                &owner.principal_token,
            )
            .unwrap();
        let child = session
            .mint_principal_credential(
                &owner_auth,
                MintPrincipalRequest {
                    authority_id: None,
                    kind: PrincipalKind::Agent,
                    name: "No Peer Read".to_string(),
                    role: Some("agent".to_string()),
                    parent_principal_id: None,
                    capabilities: vec![CredentialCapability::MutateCoordination],
                    profile: serde_json::json!({}),
                },
            )
            .unwrap();
        let host = Arc::new(QueryHost::with_session_and_limits_and_features(
            session,
            prism_query::QueryLimits::default(),
            PrismMcpFeatures::default(),
        ));
        let router = routes(PeerRuntimeAppState { host, root });
        let response = router
            .oneshot(
                Request::builder()
                    .uri(prism_core::PEER_RUNTIME_READ_PATH)
                    .method("POST")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "credentialId": child.credential.credential_id.0,
                            "principalToken": child.principal_token,
                            "kind": "runtime_diagnostics"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn peer_runtime_read_returns_bounded_runtime_diagnostics() {
        let root = init_git_workspace("task/peer-runtime-read");
        let session = hydrate_workspace_session_with_options(
            &root,
            WorkspaceSessionOptions {
                coordination: true,
                shared_runtime: default_workspace_shared_runtime(&root).unwrap(),
                hydrate_persisted_projections: false,
                hydrate_persisted_co_change: false,
            },
        )
        .unwrap();
        let owner = session
            .bootstrap_owner_principal(BootstrapOwnerInput {
                authority_id: None,
                name: "Owner".to_string(),
                role: Some("owner".to_string()),
            })
            .unwrap();
        let uri_path = PrismPaths::for_workspace_root(&root)
            .unwrap()
            .mcp_http_uri_path()
            .unwrap();
        std::fs::create_dir_all(uri_path.parent().unwrap()).unwrap();
        std::fs::write(&uri_path, "http://127.0.0.1:52695/mcp").unwrap();
        sync_live_runtime_descriptor(&root).unwrap();
        let endpoint = local_peer_runtime_endpoint(&root).unwrap().unwrap();

        let owner_auth = session
            .authenticate_principal_credential(
                &owner.credential.credential_id,
                &owner.principal_token,
            )
            .unwrap();
        let peer_reader = session
            .mint_principal_credential(
                &owner_auth,
                MintPrincipalRequest {
                    authority_id: None,
                    kind: PrincipalKind::Agent,
                    name: "Peer Reader".to_string(),
                    role: Some("peer_reader".to_string()),
                    parent_principal_id: None,
                    capabilities: vec![CredentialCapability::ReadPeerRuntime],
                    profile: serde_json::json!({}),
                },
            )
            .unwrap();
        let host = Arc::new(QueryHost::with_session_and_limits_and_features(
            session,
            prism_query::QueryLimits::default(),
            PrismMcpFeatures::default(),
        ));
        let router = routes(PeerRuntimeAppState {
            host,
            root: root.clone(),
        });
        let response = router
            .oneshot(
                Request::builder()
                    .uri(prism_core::PEER_RUNTIME_READ_PATH)
                    .method("POST")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "credentialId": peer_reader.credential.credential_id.0,
                            "principalToken": peer_reader.principal_token,
                            "kind": "runtime_diagnostics"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["kind"], "runtime_diagnostics");
        assert_eq!(
            payload["sharedCoordinationRef"]["runtimeDescriptorCount"],
            Value::from(1)
        );
        assert_eq!(payload["runtimeDescriptor"]["peerEndpoint"], endpoint);
        assert!(payload["runtimeDescriptor"]["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "bounded_peer_reads"));
    }
}
