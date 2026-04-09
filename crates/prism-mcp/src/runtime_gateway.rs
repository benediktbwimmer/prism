use std::path::{Path, PathBuf};

use anyhow::Result;
use axum::{http::StatusCode, Json};
use prism_coordination::{RuntimeDescriptor, RuntimeDescriptorCapability};
use prism_core::{
    configured_coordination_authority_store_provider, runtime_query_endpoint,
    shared_coordination_ref_diagnostics, shared_coordination_ref_diagnostics_with_provider,
    CoordinationAuthorityStoreProvider, CoordinationReadConsistency, RuntimeDescriptorQuery,
    WorkspaceSession,
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
