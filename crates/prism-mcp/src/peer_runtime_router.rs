use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use prism_coordination::{RuntimeDescriptor, RuntimeDescriptorCapability};
use prism_core::{
    configured_coordination_authority_store_provider, local_runtime_id, runtime_query_endpoint,
    shared_coordination_ref_diagnostics, shared_coordination_ref_diagnostics_with_provider,
    CoordinationAuthorityStoreProvider, CoordinationReadConsistency, CredentialsFile, PrismPaths,
    RuntimeDescriptorQuery, WorkspaceSession, PEER_RUNTIME_QUERY_PATH,
};
use prism_ir::{CredentialCapability, CredentialId};
use prism_js::QueryDiagnostic;
use prism_js::QueryEnvelope;
use serde::{Deserialize, Serialize};

use crate::remote_runtime_query_error;
use crate::runtime_views::runtime_status;
use crate::trust_surface::{
    peer_runtime_auth_failed_response, peer_runtime_capability_denied_response,
};
use crate::{QueryHost, QueryLanguage};

const PEER_QUERY_TIMEOUT: Duration = Duration::from_secs(20);
pub(crate) const MAX_PEER_QUERY_CODE_CHARS: usize = 24_000;
const STALE_RUNTIME_DESCRIPTOR_AFTER_SECS: u64 = 15 * 60;

#[derive(Clone)]
pub(crate) struct PeerRuntimeAppState {
    pub(crate) host: Arc<QueryHost>,
    pub(crate) root: PathBuf,
}

pub(crate) fn routes(state: PeerRuntimeAppState) -> Router {
    Router::new()
        .route(PEER_RUNTIME_QUERY_PATH, post(peer_runtime_query))
        .with_state(state)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PeerRuntimeQueryRequest {
    credential_id: String,
    principal_token: String,
    runtime_id: String,
    code: String,
    language: QueryLanguage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PeerRuntimeQueryResponse {
    pub(crate) runtime_id: String,
    pub(crate) result: QueryEnvelope,
}

#[derive(Debug, Clone)]
pub(crate) struct RemotePrismQueryResult {
    #[allow(dead_code)]
    pub(crate) runtime_descriptor: RuntimeDescriptor,
    pub(crate) response: PeerRuntimeQueryResponse,
}

#[derive(Debug, Clone)]
struct LocalPeerReadCredential {
    credential_id: String,
    principal_token: String,
}

async fn peer_runtime_query(
    State(state): State<PeerRuntimeAppState>,
    Json(request): Json<PeerRuntimeQueryRequest>,
) -> Result<Json<PeerRuntimeQueryResponse>, (StatusCode, Json<serde_json::Value>)> {
    let workspace = state
        .host
        .workspace_session()
        .ok_or_else(|| service_error("workspace-backed runtime session is unavailable"))?;
    validate_peer_query_request(&request)?;
    authenticate_peer_read(workspace, &request)?;

    let local_runtime_id = local_runtime_id(&state.root);
    if request.runtime_id != local_runtime_id {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "code": "peer_runtime_not_found",
                "message": "requested runtime descriptor is not served by this daemon",
                "runtimeId": request.runtime_id,
            })),
        ));
    }

    let host = Arc::clone(&state.host);
    let session = host.peer_query_session();
    let code = request.code;
    let language = request.language;
    let mut envelope = tokio::task::spawn_blocking(move || host.execute(session, &code, language))
        .await
        .map_err(|error| service_error(format!("peer query execution join failed: {error}")))?
        .map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "code": "peer_runtime_query_failed",
                    "message": error.to_string(),
                    "runtimeId": request.runtime_id,
                })),
            )
        })?;
    envelope
        .diagnostics
        .push(peer_enriched_diagnostic(&local_runtime_id));

    let _ = runtime_status(&state.host).map_err(|error| service_error(error.to_string()))?;

    Ok(Json(PeerRuntimeQueryResponse {
        runtime_id: local_runtime_id,
        result: envelope,
    }))
}

fn authenticate_peer_read(
    workspace: &WorkspaceSession,
    request: &PeerRuntimeQueryRequest,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let authenticated = workspace
        .authenticate_principal_credential(
            &CredentialId::new(request.credential_id.clone()),
            &request.principal_token,
        )
        .map_err(|error| {
            peer_runtime_auth_failed_response(&request.credential_id, &error.to_string())
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
        return Err(peer_runtime_capability_denied_response(
            &request.credential_id,
        ));
    }
    Ok(())
}

fn validate_peer_query_request(
    request: &PeerRuntimeQueryRequest,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if request.runtime_id.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "code": "remote_runtime_id_required",
                "message": "runtimeId must be a non-empty string",
            })),
        ));
    }
    if request.code.chars().count() > MAX_PEER_QUERY_CODE_CHARS {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "code": "peer_runtime_query_too_large",
                "message": format!(
                    "remote prism_query exceeded the {MAX_PEER_QUERY_CODE_CHARS} character limit"
                ),
                "runtimeId": request.runtime_id,
                "maxChars": MAX_PEER_QUERY_CODE_CHARS,
            })),
        ));
    }
    Ok(())
}

fn peer_enriched_diagnostic(runtime_id: &str) -> QueryDiagnostic {
    QueryDiagnostic {
        code: "peer_enriched".to_string(),
        message: format!(
            "Result was served by peer runtime `{runtime_id}` and is peer-enriched, not shared-authoritative."
        ),
        data: Some(serde_json::json!({
            "authorityClass": "peer_enriched",
            "runtimeId": runtime_id,
        })),
    }
}

pub(crate) fn execute_remote_prism_query(
    root: &Path,
    runtime_id: &str,
    code: &str,
    language: QueryLanguage,
) -> Result<RemotePrismQueryResult> {
    execute_remote_prism_query_with_provider(root, None, runtime_id, code, language)
}

pub(crate) fn execute_remote_prism_query_with_provider(
    root: &Path,
    authority_store_provider: Option<&CoordinationAuthorityStoreProvider>,
    runtime_id: &str,
    code: &str,
    language: QueryLanguage,
) -> Result<RemotePrismQueryResult> {
    if code.chars().count() > MAX_PEER_QUERY_CODE_CHARS {
        return Err(remote_runtime_query_error(
            "peer_runtime_query_too_large",
            Some(runtime_id),
            format!(
                "remote prism_query for runtime `{runtime_id}` exceeded the {MAX_PEER_QUERY_CODE_CHARS} character limit"
            ),
            format!(
                "Keep runtime-targeted prism_query snippets under {MAX_PEER_QUERY_CODE_CHARS} characters, or split the read into smaller calls."
            ),
        ));
    }
    let descriptor = resolve_runtime_descriptor(root, authority_store_provider, runtime_id)?;
    if descriptor
        .last_seen_at
        .saturating_add(STALE_RUNTIME_DESCRIPTOR_AFTER_SECS)
        < current_timestamp_secs()
    {
        return Err(remote_runtime_query_error(
            "remote_runtime_descriptor_stale",
            Some(runtime_id),
            format!(
                "runtime `{runtime_id}` has a stale shared descriptor from {}",
                descriptor.last_seen_at
            ),
            "Wait for the peer to refresh its shared runtime descriptor, or pick a different runtime id.",
        ));
    }
    if !descriptor
        .capabilities
        .contains(&RuntimeDescriptorCapability::BoundedPeerReads)
    {
        return Err(remote_runtime_query_error(
            "remote_runtime_capability_denied",
            Some(runtime_id),
            format!("runtime `{runtime_id}` does not advertise bounded peer reads"),
            "Choose a runtime that advertises `bounded_peer_reads`, or query the local runtime instead.",
        ));
    }
    let endpoint = runtime_query_endpoint(&descriptor).ok_or_else(|| {
        remote_runtime_query_error(
            "remote_runtime_endpoint_missing",
            Some(runtime_id),
            format!("runtime `{runtime_id}` does not publish a query endpoint"),
            "Set a peer or public endpoint for that runtime, or target a different runtime id.",
        )
    })?;
    let credential = resolve_local_peer_read_credential(root)?;
    let client = reqwest::blocking::Client::builder()
        .timeout(PEER_QUERY_TIMEOUT)
        .build()
        .context("failed to build peer runtime HTTP client")?;
    let request = PeerRuntimeQueryRequest {
        credential_id: credential.credential_id,
        principal_token: credential.principal_token,
        runtime_id: runtime_id.to_string(),
        code: code.to_string(),
        language,
    };
    let secondary_endpoint = descriptor
        .public_endpoint
        .as_deref()
        .zip(descriptor.peer_endpoint.as_deref())
        .and_then(|(public, peer)| (endpoint == public && public != peer).then_some(peer));
    let payload = match query_peer_runtime_endpoint(&client, endpoint, runtime_id, &request) {
        Ok(payload) => payload,
        Err(primary_error) if primary_error.retryable => {
            if let Some(fallback_endpoint) = secondary_endpoint {
                query_peer_runtime_endpoint(&client, fallback_endpoint, runtime_id, &request)
                    .map_err(|fallback_error| fallback_error.error)?
            } else {
                return Err(primary_error.error);
            }
        }
        Err(primary_error) => return Err(primary_error.error),
    };
    if payload.runtime_id != descriptor.runtime_id {
        return Err(remote_runtime_query_error(
            "remote_runtime_descriptor_stale",
            Some(runtime_id),
            format!(
                "shared coordination resolved runtime `{runtime_id}`, but the peer responded as `{}`",
                payload.runtime_id
            ),
            "Refresh shared coordination so the runtime descriptor matches the live peer, or target the newer runtime id.",
        ));
    }
    Ok(RemotePrismQueryResult {
        runtime_descriptor: descriptor,
        response: payload,
    })
}

struct PeerRuntimeQueryFailure {
    error: anyhow::Error,
    retryable: bool,
}

fn query_peer_runtime_endpoint(
    client: &reqwest::blocking::Client,
    endpoint: &str,
    runtime_id: &str,
    request: &PeerRuntimeQueryRequest,
) -> std::result::Result<PeerRuntimeQueryResponse, PeerRuntimeQueryFailure> {
    let response = client
        .post(endpoint)
        .json(request)
        .send()
        .map_err(|error| PeerRuntimeQueryFailure {
            error: remote_runtime_query_error(
                "remote_runtime_unreachable",
                Some(runtime_id),
                format!("failed to contact peer runtime `{runtime_id}` at {endpoint}: {error}"),
                "Check the peer endpoint or public URL in shared coordination, then retry. If the peer is offline, fall back to local reads.",
            ),
            retryable: true,
        })?;
    let status = response.status();
    if !status.is_success() {
        let body_text = response.text().unwrap_or_default();
        let body = serde_json::from_str::<serde_json::Value>(&body_text)
            .unwrap_or_else(|_| serde_json::json!({ "code": "peer_runtime_request_failed" }));
        let message = body
            .get("message")
            .and_then(|value| value.as_str())
            .unwrap_or_else(|| {
                let trimmed = body_text.trim();
                if trimmed.is_empty() {
                    "peer runtime request failed"
                } else {
                    trimmed
                }
            });
        let code = body
            .get("code")
            .and_then(|value| value.as_str())
            .unwrap_or("peer_runtime_request_failed");
        let mapped_code = match code {
            "peer_runtime_not_found" => "remote_runtime_descriptor_stale",
            "peer_runtime_capability_denied" => "remote_runtime_capability_denied",
            "peer_runtime_query_too_large" => "peer_runtime_query_too_large",
            "peer_runtime_auth_failed" => "remote_runtime_auth_failed",
            "peer_runtime_unavailable" => "remote_runtime_unreachable",
            _ => "remote_runtime_query_failed",
        };
        let next_action = match mapped_code {
            "remote_runtime_descriptor_stale" => {
                "Refresh shared coordination so the runtime descriptor matches the live peer, or pick a different runtime id."
            }
            "remote_runtime_capability_denied" => {
                "Use a credential with `read_peer_runtime`, or query the local runtime instead."
            }
            "peer_runtime_query_too_large" => {
                "Reduce the size of the remote prism_query snippet, or split the read into smaller calls."
            }
            "remote_runtime_auth_failed" => {
                "Refresh the active local credential or rebind the bridge to a credential that the peer accepts."
            }
            "remote_runtime_unreachable" => {
                "Check whether the peer endpoint is reachable, or fall back to local/runtime-independent reads."
            }
            _ => "Inspect the peer runtime error and retry with a smaller or simpler query.",
        };
        let retryable = !matches!(
            mapped_code,
            "remote_runtime_capability_denied"
                | "peer_runtime_query_too_large"
                | "remote_runtime_auth_failed"
        );
        return Err(PeerRuntimeQueryFailure {
            error: remote_runtime_query_error(
                mapped_code,
                Some(runtime_id),
                format!("peer runtime `{runtime_id}` query failed ({status} {code}): {message}"),
                next_action,
            ),
            retryable,
        });
    }
    response
        .json::<PeerRuntimeQueryResponse>()
        .map_err(|error| PeerRuntimeQueryFailure {
            error: remote_runtime_query_error(
                "remote_runtime_response_invalid",
                Some(runtime_id),
                format!("failed to decode peer runtime query response from {endpoint}: {error}"),
                "Retry the query. If it keeps failing, inspect the peer runtime version and response format.",
            ),
            retryable: true,
        })
}

fn resolve_local_peer_read_credential(root: &Path) -> Result<LocalPeerReadCredential> {
    let credentials_path = PrismPaths::for_workspace_root(root)?.credentials_path()?;
    let credentials = CredentialsFile::load(&credentials_path).with_context(|| {
        format!(
            "failed to load credentials from {}",
            credentials_path.display()
        )
    })?;
    let profile = credentials
        .find_by_selector(None, None, None)
        .with_context(|| {
            format!(
                "no active local PRISM credential is available for peer reads in {}",
                credentials_path.display()
            )
        })?;
    Ok(LocalPeerReadCredential {
        credential_id: profile.credential_id.clone(),
        principal_token: profile.principal_token.clone(),
    })
}

fn resolve_runtime_descriptor(
    root: &Path,
    authority_store_provider: Option<&CoordinationAuthorityStoreProvider>,
    runtime_id: &str,
) -> Result<RuntimeDescriptor> {
    let diagnostics = match authority_store_provider {
        Some(provider) => shared_coordination_ref_diagnostics_with_provider(root, provider)?,
        None => shared_coordination_ref_diagnostics(root)?,
    };
    if let Some(diagnostics) = diagnostics {
        if !diagnostics.authoritative_hydration_allowed {
            return Err(remote_runtime_query_error(
                "remote_runtime_shared_ref_degraded",
                Some(runtime_id),
                diagnostics
                    .verification_error
                    .unwrap_or_else(|| "shared coordination verification is degraded".to_string()),
                diagnostics.repair_hint.as_deref().unwrap_or(
                    "Repair or republish the shared coordination ref before relying on peer runtime routing.",
                ),
            ));
        }
        if let Some(descriptor) = diagnostics
            .runtime_descriptors
            .into_iter()
            .find(|descriptor| descriptor.runtime_id == runtime_id)
        {
            return Ok(descriptor);
        }
    }

    let provider = authority_store_provider
        .cloned()
        .unwrap_or(configured_coordination_authority_store_provider(root)?);
    let store = provider.open(root)?;
    let runtime_descriptors = store
        .list_runtime_descriptors(RuntimeDescriptorQuery {
            consistency: CoordinationReadConsistency::Strong,
        })?
        .value
        .unwrap_or_default();
    if runtime_descriptors.is_empty() {
        return Err(remote_runtime_query_error(
            "remote_runtime_shared_ref_unavailable",
            Some(runtime_id),
            "shared coordination ref is unavailable".to_string(),
            "Restore shared coordination connectivity, or query the local runtime instead.",
        ));
    }
    runtime_descriptors
        .into_iter()
        .find(|descriptor| descriptor.runtime_id == runtime_id)
        .ok_or_else(|| {
            remote_runtime_query_error(
                "remote_runtime_descriptor_missing",
                Some(runtime_id),
                format!("runtime `{runtime_id}` is not present in shared coordination"),
                "Check the runtime id or wait for the peer to publish its descriptor.",
            )
        })
}

fn current_timestamp_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
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
    use std::net::SocketAddr;
    use std::path::Path;
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::Arc;

    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use prism_core::{
        default_workspace_shared_runtime, hydrate_workspace_session_with_options, local_runtime_id,
        sync_live_runtime_descriptor, BootstrapOwnerInput, CredentialProfile, CredentialsFile,
        MintPrincipalRequest, PrismPaths, WorkspaceSessionOptions,
    };
    use prism_ir::{CredentialCapability, PrincipalKind};
    use serde_json::Value;
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;
    use tower::util::ServiceExt;

    use crate::peer_runtime_router::MAX_PEER_QUERY_CODE_CHARS;
    use crate::query_errors::QueryExecutionError;
    use crate::tests_support::{credentials_test_lock, temp_workspace};
    use crate::{PrismMcpFeatures, QueryHost, QueryLanguage};

    use super::{execute_remote_prism_query, routes, PeerRuntimeAppState, PeerRuntimeQueryRequest};

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

    fn peer_runtime_session(root: &Path) -> prism_core::WorkspaceSession {
        hydrate_workspace_session_with_options(
            root,
            WorkspaceSessionOptions {
                runtime_mode: prism_core::PrismRuntimeMode::Full,
                shared_runtime: default_workspace_shared_runtime(root).unwrap(),
                hydrate_persisted_projections: false,
                hydrate_persisted_co_change: false,
            },
        )
        .unwrap()
    }

    fn peer_runtime_features() -> PrismMcpFeatures {
        PrismMcpFeatures::full().with_internal_developer(true)
    }

    fn persist_active_credential(
        root: &Path,
        profile: &str,
        principal_id: &str,
        credential_id: &str,
        principal_token: &str,
    ) {
        let credentials_path = PrismPaths::for_workspace_root(root)
            .unwrap()
            .credentials_path()
            .unwrap();
        let mut credentials = CredentialsFile::load(&credentials_path).unwrap();
        credentials.upsert_profile(
            CredentialProfile {
                profile: profile.to_string(),
                authority_id: "local-daemon".to_string(),
                principal_id: principal_id.to_string(),
                credential_id: credential_id.to_string(),
                principal_token: principal_token.to_string(),
                encrypted_secret: None,
                principal_metadata: None,
                credential_metadata: None,
            },
            true,
        );
        credentials.save(&credentials_path).unwrap();
    }

    async fn serve_peer_router(
        root: PathBuf,
        host: Arc<QueryHost>,
    ) -> (SocketAddr, oneshot::Sender<()>) {
        let router = routes(PeerRuntimeAppState { host, root });
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        tokio::spawn(async move {
            let _ = axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await;
        });
        (addr, shutdown_tx)
    }

    #[tokio::test]
    async fn peer_runtime_query_requires_explicit_capability() {
        let _guard = credentials_test_lock();
        let root = init_git_workspace("task/peer-runtime-query-denied");
        let session = peer_runtime_session(&root);
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
                    kind: PrincipalKind::Service,
                    name: "No Peer Read".to_string(),
                    role: Some("peer_query_service".to_string()),
                    parent_principal_id: None,
                    capabilities: vec![CredentialCapability::MutateCoordination],
                    profile: serde_json::json!({}),
                },
            )
            .unwrap();
        persist_active_credential(
            &root,
            "peer-reader",
            &child.principal.principal_id.0,
            &child.credential.credential_id.0,
            &child.principal_token,
        );
        let host = Arc::new(QueryHost::with_session_and_limits_and_features(
            session,
            prism_query::QueryLimits::default(),
            peer_runtime_features(),
        ));
        let router = routes(PeerRuntimeAppState {
            host,
            root: root.clone(),
        });
        let response = router
            .oneshot(
                Request::builder()
                    .uri(prism_core::PEER_RUNTIME_QUERY_PATH)
                    .method("POST")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_string(&PeerRuntimeQueryRequest {
                            credential_id: child.credential.credential_id.0.to_string(),
                            principal_token: child.principal_token,
                            runtime_id: local_runtime_id(&root),
                            code: "return { ok: true };".to_string(),
                            language: crate::QueryLanguage::Ts,
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn peer_runtime_query_executes_prism_query() {
        let _guard = credentials_test_lock();
        let root = init_git_workspace("task/peer-runtime-query");
        let session = peer_runtime_session(&root);
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
                    kind: PrincipalKind::Service,
                    name: "Peer Reader".to_string(),
                    role: Some("peer_reader_service".to_string()),
                    parent_principal_id: None,
                    capabilities: vec![CredentialCapability::ReadPeerRuntime],
                    profile: serde_json::json!({}),
                },
            )
            .unwrap();
        persist_active_credential(
            &root,
            "peer-reader",
            &child.principal.principal_id.0,
            &child.credential.credential_id.0,
            &child.principal_token,
        );
        let host = Arc::new(QueryHost::with_session_and_limits_and_features(
            session,
            prism_query::QueryLimits::default(),
            peer_runtime_features(),
        ));
        let router = routes(PeerRuntimeAppState {
            host,
            root: root.clone(),
        });
        let response = router
            .oneshot(
                Request::builder()
                    .uri(prism_core::PEER_RUNTIME_QUERY_PATH)
                    .method("POST")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_string(&PeerRuntimeQueryRequest {
                            credential_id: child.credential.credential_id.0.to_string(),
                            principal_token: child.principal_token,
                            runtime_id: local_runtime_id(&root),
                            code: "return { ok: true };".to_string(),
                            language: crate::QueryLanguage::Ts,
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["runtimeId"], Value::from(local_runtime_id(&root)));
        assert_eq!(payload["result"]["result"]["ok"], Value::Bool(true));
        assert!(payload["result"]["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|diagnostic| {
                diagnostic["code"] == Value::from("peer_enriched")
                    && diagnostic["data"]["authorityClass"] == Value::from("peer_enriched")
            }));
    }

    #[tokio::test]
    async fn execute_remote_prism_query_resolves_runtime_id_from_shared_ref() {
        let _guard = credentials_test_lock();
        let root = init_git_workspace("task/peer-runtime-client");
        let session = peer_runtime_session(&root);
        let owner = session
            .bootstrap_owner_principal(BootstrapOwnerInput {
                authority_id: None,
                name: "Owner".to_string(),
                role: Some("owner".to_string()),
            })
            .unwrap();
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
                    kind: PrincipalKind::Service,
                    name: "Peer Reader".to_string(),
                    role: Some("peer_reader_service".to_string()),
                    parent_principal_id: None,
                    capabilities: vec![CredentialCapability::ReadPeerRuntime],
                    profile: serde_json::json!({}),
                },
            )
            .unwrap();
        persist_active_credential(
            &root,
            "peer-reader",
            &child.principal.principal_id.0,
            &child.credential.credential_id.0,
            &child.principal_token,
        );
        let host = Arc::new(QueryHost::with_session_and_limits_and_features(
            session,
            prism_query::QueryLimits::default(),
            peer_runtime_features(),
        ));

        let (addr, shutdown) = serve_peer_router(root.clone(), host).await;
        let uri_path = PrismPaths::for_workspace_root(&root)
            .unwrap()
            .mcp_http_uri_path()
            .unwrap();
        std::fs::create_dir_all(uri_path.parent().unwrap()).unwrap();
        std::fs::write(&uri_path, format!("http://{addr}/mcp")).unwrap();
        sync_live_runtime_descriptor(&root).unwrap();

        let runtime_id = local_runtime_id(&root);
        let root_for_query = root.clone();
        let runtime_id_for_query = runtime_id.clone();
        let result = tokio::task::spawn_blocking(move || {
            execute_remote_prism_query(
                &root_for_query,
                &runtime_id_for_query,
                "return { peer: true };",
                crate::QueryLanguage::Ts,
            )
        })
        .await
        .unwrap()
        .unwrap();
        assert_eq!(result.response.runtime_id, runtime_id);
        assert_eq!(result.response.result.result["peer"], Value::Bool(true));
        assert!(result
            .response
            .result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "peer_enriched"));
        assert!(result
            .runtime_descriptor
            .capabilities
            .contains(&prism_coordination::RuntimeDescriptorCapability::BoundedPeerReads));

        let _ = shutdown.send(());
    }

    #[tokio::test]
    async fn execute_remote_prism_query_falls_back_to_peer_endpoint_when_public_url_is_offline() {
        let _guard = credentials_test_lock();
        let root = init_git_workspace("task/peer-runtime-public-fallback");
        let session = peer_runtime_session(&root);
        let owner = session
            .bootstrap_owner_principal(BootstrapOwnerInput {
                authority_id: None,
                name: "Owner".to_string(),
                role: Some("owner".to_string()),
            })
            .unwrap();
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
                    kind: PrincipalKind::Service,
                    name: "Peer Reader".to_string(),
                    role: Some("peer_reader_service".to_string()),
                    parent_principal_id: None,
                    capabilities: vec![CredentialCapability::ReadPeerRuntime],
                    profile: serde_json::json!({}),
                },
            )
            .unwrap();
        persist_active_credential(
            &root,
            "peer-reader",
            &child.principal.principal_id.0,
            &child.credential.credential_id.0,
            &child.principal_token,
        );
        let host = Arc::new(QueryHost::with_session_and_limits_and_features(
            session,
            prism_query::QueryLimits::default(),
            peer_runtime_features(),
        ));

        let (addr, shutdown) = serve_peer_router(root.clone(), host).await;
        let paths = PrismPaths::for_workspace_root(&root).unwrap();
        let uri_path = paths.mcp_http_uri_path().unwrap();
        let public_url_path = paths.mcp_public_url_path().unwrap();
        std::fs::create_dir_all(uri_path.parent().unwrap()).unwrap();
        std::fs::create_dir_all(public_url_path.parent().unwrap()).unwrap();
        std::fs::write(&uri_path, format!("http://{addr}/mcp")).unwrap();
        std::fs::write(&public_url_path, "http://127.0.0.1:9/peer/query\n").unwrap();
        sync_live_runtime_descriptor(&root).unwrap();

        let runtime_id = local_runtime_id(&root);
        let root_for_query = root.clone();
        let runtime_id_for_query = runtime_id.clone();
        let result = tokio::task::spawn_blocking(move || {
            execute_remote_prism_query(
                &root_for_query,
                &runtime_id_for_query,
                "return { peerFallback: true };",
                crate::QueryLanguage::Ts,
            )
        })
        .await
        .unwrap()
        .unwrap();
        assert_eq!(result.response.runtime_id, runtime_id);
        assert_eq!(
            result.response.result.result["peerFallback"],
            Value::Bool(true)
        );

        let _ = shutdown.send(());
    }

    #[tokio::test]
    async fn prism_query_from_runtime_executes_remote_runtime_and_file_reads() {
        let _guard = credentials_test_lock();
        let root = init_git_workspace("task/peer-runtime-chaining");
        std::fs::write(root.join("notes.txt"), "peer-runtime-notes").unwrap();
        let session = peer_runtime_session(&root);
        let owner = session
            .bootstrap_owner_principal(BootstrapOwnerInput {
                authority_id: None,
                name: "Owner".to_string(),
                role: Some("owner".to_string()),
            })
            .unwrap();
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
                    kind: PrincipalKind::Service,
                    name: "Peer Reader".to_string(),
                    role: Some("peer_reader_service".to_string()),
                    parent_principal_id: None,
                    capabilities: vec![CredentialCapability::ReadPeerRuntime],
                    profile: serde_json::json!({}),
                },
            )
            .unwrap();
        persist_active_credential(
            &root,
            "peer-reader",
            &child.principal.principal_id.0,
            &child.credential.credential_id.0,
            &child.principal_token,
        );

        let remote_host = Arc::new(QueryHost::with_session_and_limits_and_features(
            session,
            prism_query::QueryLimits::default(),
            peer_runtime_features(),
        ));
        let local_host = Arc::new(QueryHost::with_session_and_limits_and_features(
            peer_runtime_session(&root),
            prism_query::QueryLimits::default(),
            peer_runtime_features(),
        ));
        let (addr, shutdown) = serve_peer_router(root.clone(), Arc::clone(&remote_host)).await;
        let uri_path = PrismPaths::for_workspace_root(&root)
            .unwrap()
            .mcp_http_uri_path()
            .unwrap();
        std::fs::create_dir_all(uri_path.parent().unwrap()).unwrap();
        std::fs::write(&uri_path, format!("http://{addr}/mcp")).unwrap();
        sync_live_runtime_descriptor(&root).unwrap();

        let runtime_id = local_runtime_id(&root);
        let query_session = local_host.peer_query_session();
        let query_host = Arc::clone(&local_host);
        let query = format!(
            r#"
const runtimeId = {};
const status = prism.from(runtimeId).runtime.status();
const slice = prism.from(runtimeId).file("notes.txt").read();
return {{ root: status.root, text: slice.text }};
"#,
            serde_json::to_string(&runtime_id).unwrap()
        );
        let envelope = tokio::task::spawn_blocking(move || {
            query_host.execute(query_session, &query, crate::QueryLanguage::Ts)
        })
        .await
        .unwrap()
        .unwrap();
        let expected_root = std::fs::canonicalize(&root)
            .unwrap_or_else(|_| root.clone())
            .display()
            .to_string();
        assert_eq!(envelope.result["root"], Value::from(expected_root));
        assert_eq!(envelope.result["text"], Value::from("peer-runtime-notes"));
        assert!(envelope
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "peer_enriched"));

        let _ = shutdown.send(());
    }

    #[tokio::test]
    async fn prism_query_from_runtime_rejects_empty_runtime_id() {
        let _guard = credentials_test_lock();
        let root = init_git_workspace("task/peer-runtime-empty-id");
        let host = Arc::new(QueryHost::with_session_and_limits_and_features(
            peer_runtime_session(&root),
            prism_query::QueryLimits::default(),
            peer_runtime_features(),
        ));
        let session = host.peer_query_session();
        let error = tokio::task::spawn_blocking(move || {
            host.execute(
                session,
                r#"return prism.from("").runtime.status();"#,
                QueryLanguage::Ts,
            )
        })
        .await
        .unwrap()
        .unwrap_err();
        let query_error = error.downcast_ref::<QueryExecutionError>().unwrap();
        assert_eq!(query_error.code(), Some("remote_runtime_id_required"));
    }

    #[test]
    fn execute_remote_prism_query_rejects_oversized_code() {
        let root = init_git_workspace("task/peer-runtime-oversized-query");
        let error = execute_remote_prism_query(
            &root,
            "runtime-oversized",
            &"x".repeat(MAX_PEER_QUERY_CODE_CHARS + 1),
            QueryLanguage::Ts,
        )
        .unwrap_err();
        let query_error = error.downcast_ref::<QueryExecutionError>().unwrap();
        assert_eq!(query_error.code(), Some("peer_runtime_query_too_large"));
    }
}
