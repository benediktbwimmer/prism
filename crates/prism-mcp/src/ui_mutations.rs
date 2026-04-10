use std::path::Path;

use axum::Json;
use axum::http::StatusCode;
use prism_core::PrismPaths;
use rmcp::ErrorData as McpError;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::ui_credentials::{load_ui_credentials, resolve_ui_credential_profile};
use crate::{PrismMutationArgs, PrismMutationCredentialArgs};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismUiMutateRequest {
    pub(crate) action: String,
    #[serde(default)]
    pub(crate) input: Value,
}

pub(crate) fn resolve_ui_mutation_args(
    root: &Path,
    workspace: Option<&prism_core::WorkspaceSession>,
    request: PrismUiMutateRequest,
) -> Result<PrismMutationArgs, (StatusCode, Json<Value>)> {
    let credential = resolve_active_local_mutation_credential(root, workspace)?;
    serde_json::from_value::<PrismMutationArgs>(json!({
        "action": request.action,
        "credential": {
            "credentialId": credential.credential_id,
            "principalToken": credential.principal_token,
        },
        "input": request.input,
    }))
    .map_err(|error| {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({
                "code": "ui_mutation_invalid_request",
                "message": "operator console mutation request is invalid",
                "error": error.to_string(),
            })),
        )
    })
}

pub(crate) fn map_ui_mutation_error(error: McpError) -> (StatusCode, Json<Value>) {
    let payload = serde_json::to_value(&error).unwrap_or_else(|serialize_error| {
        json!({
            "code": -32603,
            "message": error.to_string(),
            "data": {
                "serializationError": serialize_error.to_string(),
            }
        })
    });
    let status = match payload.get("code").and_then(Value::as_i64) {
        Some(-32601) => StatusCode::NOT_FOUND,
        Some(-32602) => StatusCode::UNPROCESSABLE_ENTITY,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, Json(json!({ "error": payload })))
}

fn resolve_active_local_mutation_credential(
    root: &Path,
    workspace: Option<&prism_core::WorkspaceSession>,
) -> Result<PrismMutationCredentialArgs, (StatusCode, Json<Value>)> {
    let credentials_path = PrismPaths::for_workspace_root(root)
        .and_then(|paths| paths.credentials_path())
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "code": "ui_mutation_credentials_path_failed",
                    "message": error.to_string(),
                })),
            )
        })?;
    let credentials = load_ui_credentials(root).map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "code": "ui_mutation_credentials_load_failed",
                "message": format!(
                    "failed to load local PRISM credentials from {}: {error}",
                    credentials_path.display()
                ),
            })),
        )
    })?;
    let profile = resolve_ui_credential_profile(&credentials, workspace).map_err(|error| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "code": "ui_mutation_local_identity_unavailable",
                "message": format!(
                    "no valid local PRISM credential is available for UI mutations in {}: {error}",
                    credentials_path.display()
                ),
                "nextAction": "Run `prism auth login` or bootstrap the local owner principal before using the operator console mutate endpoint.",
            })),
        )
    })?;
    Ok(PrismMutationCredentialArgs {
        credential_id: profile.credential_id.clone(),
        principal_token: profile.principal_token.clone(),
    })
}
