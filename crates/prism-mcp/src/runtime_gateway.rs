use std::path::{Path, PathBuf};

use anyhow::Result;
use axum::{http::StatusCode, Json};
use prism_coordination::{RuntimeDescriptor, RuntimeDescriptorCapability};
use prism_core::{
    configured_coordination_authority_store_provider,
    coordination_authority_diagnostics_with_provider, runtime_query_endpoint,
    CoordinationAuthorityBackendDetails, CoordinationAuthorityStoreProvider,
    CoordinationReadConsistency, RuntimeDescriptorQuery, WorkspaceSession,
};
use prism_ir::{CredentialCapability, CredentialId};

use crate::peer_runtime_router::{
    execute_remote_prism_query_with_provider, RemotePrismQueryResult,
};
use crate::remote_runtime_query_error;
use crate::trust_surface::{
    peer_runtime_auth_failed_response, peer_runtime_capability_denied_response,
};
use crate::QueryLanguage;

pub(crate) const MAX_PEER_QUERY_CODE_CHARS: usize = 24_000;
const STALE_RUNTIME_DESCRIPTOR_AFTER_SECS: u64 = 15 * 60;

#[derive(Debug, Clone)]
pub(crate) struct ResolvedRemoteRuntimeTarget {
    pub(crate) runtime_descriptor: RuntimeDescriptor,
    pub(crate) endpoint: String,
    pub(crate) secondary_endpoint: Option<String>,
}

#[derive(Clone)]
pub(crate) struct WorkspaceRuntimeGateway {
    root: PathBuf,
    authority_store_provider: CoordinationAuthorityStoreProvider,
}

impl WorkspaceRuntimeGateway {
    pub(crate) fn new(
        root: PathBuf,
        authority_store_provider: CoordinationAuthorityStoreProvider,
    ) -> Self {
        Self {
            root,
            authority_store_provider,
        }
    }

    pub(crate) fn execute_remote_prism_query(
        &self,
        runtime_id: &str,
        code: &str,
        language: QueryLanguage,
    ) -> Result<RemotePrismQueryResult> {
        execute_remote_prism_query_with_provider(
            &self.root,
            Some(&self.authority_store_provider),
            runtime_id,
            code,
            language,
        )
    }

    pub(crate) fn authenticate_incoming_peer_read(
        &self,
        workspace: &WorkspaceSession,
        credential_id: &str,
        principal_token: &str,
    ) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
        authenticate_incoming_peer_read(workspace, credential_id, principal_token)
    }
}

pub(crate) fn authenticate_incoming_peer_read(
    workspace: &WorkspaceSession,
    credential_id: &str,
    principal_token: &str,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let authenticated = workspace
        .authenticate_principal_credential(
            &CredentialId::new(credential_id.to_string()),
            principal_token,
        )
        .map_err(|error| peer_runtime_auth_failed_response(credential_id, &error.to_string()))?;
    if !authenticated
        .credential
        .capabilities
        .contains(&CredentialCapability::All)
        && !authenticated
            .credential
            .capabilities
            .contains(&CredentialCapability::ReadPeerRuntime)
    {
        return Err(peer_runtime_capability_denied_response(credential_id));
    }
    Ok(())
}

pub(crate) fn validate_incoming_peer_query_request(
    runtime_id: &str,
    code: &str,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if runtime_id.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "code": "remote_runtime_id_required",
                "message": "runtimeId must be a non-empty string",
            })),
        ));
    }
    ensure_outbound_query_size(runtime_id, code).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "code": "peer_runtime_query_too_large",
                "message": error.to_string(),
                "runtimeId": runtime_id,
                "maxChars": MAX_PEER_QUERY_CODE_CHARS,
            })),
        )
    })
}

pub(crate) fn ensure_outbound_query_size(runtime_id: &str, code: &str) -> Result<()> {
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
    Ok(())
}

pub(crate) fn resolve_remote_runtime_target_for_root(
    root: &Path,
    authority_store_provider: Option<&CoordinationAuthorityStoreProvider>,
    runtime_id: &str,
) -> Result<ResolvedRemoteRuntimeTarget> {
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
                "runtime `{runtime_id}` has a stale published runtime descriptor from {}",
                descriptor.last_seen_at
            ),
            "Wait for the peer to refresh its published runtime descriptor, or pick a different runtime id.",
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
    let endpoint = runtime_query_endpoint(&descriptor)
        .map(str::to_owned)
        .ok_or_else(|| {
            remote_runtime_query_error(
                "remote_runtime_endpoint_missing",
                Some(runtime_id),
                format!("runtime `{runtime_id}` does not publish a query endpoint"),
                "Set a peer or public endpoint for that runtime, or target a different runtime id.",
            )
        })?;
    let secondary_endpoint = descriptor
        .public_endpoint
        .as_deref()
        .zip(descriptor.peer_endpoint.as_deref())
        .and_then(|(public, peer)| (endpoint == public && public != peer).then_some(peer))
        .map(ToOwned::to_owned);
    Ok(ResolvedRemoteRuntimeTarget {
        runtime_descriptor: descriptor,
        endpoint,
        secondary_endpoint,
    })
}

fn resolve_runtime_descriptor(
    root: &Path,
    authority_store_provider: Option<&CoordinationAuthorityStoreProvider>,
    runtime_id: &str,
) -> Result<RuntimeDescriptor> {
    let provider = authority_store_provider
        .cloned()
        .unwrap_or(configured_coordination_authority_store_provider(root)?);
    let diagnostics = coordination_authority_diagnostics_with_provider(root, &provider)?;
    if let CoordinationAuthorityBackendDetails::GitSharedRefs(diagnostics) =
        diagnostics.backend_details
    {
        if !diagnostics.authoritative_hydration_allowed {
            return Err(remote_runtime_query_error(
                "remote_runtime_authority_degraded",
                Some(runtime_id),
                diagnostics
                    .verification_error
                    .unwrap_or_else(|| {
                        "coordination authority verification is degraded".to_string()
                    }),
                diagnostics.repair_hint.as_deref().unwrap_or(
                    "Repair or republish the coordination authority state before relying on peer runtime routing.",
                ),
            ));
        }
    }
    let store = provider.open(root)?;
    let runtime_descriptors = store
        .list_runtime_descriptors(RuntimeDescriptorQuery {
            consistency: CoordinationReadConsistency::Strong,
        })?
        .value
        .unwrap_or_default();
    if runtime_descriptors.is_empty() {
        return Err(remote_runtime_query_error(
            "remote_runtime_authority_unavailable",
            Some(runtime_id),
            "coordination authority has no published runtime descriptors".to_string(),
            "Wait for the peer to publish its runtime descriptor, or query the local runtime instead.",
        ));
    }
    runtime_descriptors
        .into_iter()
        .find(|descriptor| descriptor.runtime_id == runtime_id)
        .ok_or_else(|| {
            remote_runtime_query_error(
                "remote_runtime_descriptor_missing",
                Some(runtime_id),
                format!(
                    "runtime `{runtime_id}` is not present in the published coordination authority descriptors"
                ),
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

#[cfg(test)]
mod tests {
    use prism_coordination::{RuntimeDescriptor, RuntimeDiscoveryMode};
    use prism_core::{
        configured_coordination_authority_store_provider, CoordinationTransactionBase,
        RuntimeDescriptorPublishRequest,
    };

    use crate::tests_support::temp_workspace;
    use crate::QueryExecutionError;

    use super::resolve_remote_runtime_target_for_root;

    fn sample_runtime_descriptor(runtime_id: &str) -> RuntimeDescriptor {
        RuntimeDescriptor {
            runtime_id: runtime_id.to_string(),
            repo_id: "repo:test".to_string(),
            worktree_id: "worktree:test".to_string(),
            principal_id: "principal:test".to_string(),
            instance_started_at: 10,
            last_seen_at: u64::MAX / 2,
            branch_ref: Some("refs/heads/main".to_string()),
            checked_out_commit: Some("abc123".to_string()),
            capabilities: vec![prism_coordination::RuntimeDescriptorCapability::BoundedPeerReads],
            discovery_mode: RuntimeDiscoveryMode::Full,
            peer_endpoint: Some("http://127.0.0.1:9001/mcp".to_string()),
            public_endpoint: Some("https://example.test/mcp".to_string()),
            peer_transport_identity: None,
            blob_snapshot_head: None,
            export_policy: None,
        }
    }

    #[test]
    fn runtime_gateway_resolves_runtime_descriptor_from_sqlite_authority_store() {
        let root = temp_workspace();
        let provider = configured_coordination_authority_store_provider(&root).unwrap();
        let store = provider.open(&root).unwrap();
        let descriptor = sample_runtime_descriptor("runtime:test");
        store
            .publish_runtime_descriptor(RuntimeDescriptorPublishRequest {
                base: CoordinationTransactionBase::LatestStrong,
                descriptor: descriptor.clone(),
            })
            .unwrap();

        let resolved =
            resolve_remote_runtime_target_for_root(&root, Some(&provider), "runtime:test").unwrap();

        assert_eq!(resolved.runtime_descriptor.runtime_id, "runtime:test");
        assert_eq!(resolved.endpoint, "https://example.test/mcp");
        assert_eq!(
            resolved.secondary_endpoint.as_deref(),
            Some("http://127.0.0.1:9001/mcp")
        );
    }

    #[test]
    fn runtime_gateway_reports_authority_unavailable_when_no_descriptors_are_published() {
        let root = temp_workspace();
        let provider = configured_coordination_authority_store_provider(&root).unwrap();

        let error =
            resolve_remote_runtime_target_for_root(&root, Some(&provider), "runtime:missing")
                .unwrap_err();
        let runtime = error.downcast::<QueryExecutionError>().unwrap();

        assert_eq!(runtime.code(), Some("remote_runtime_authority_unavailable"));
        assert!(runtime
            .to_string()
            .contains("coordination authority has no published runtime descriptors"));
    }
}
