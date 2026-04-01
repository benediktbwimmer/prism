use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use prism_core::{CredentialProfile, CredentialsFile, PrismPaths};
use rmcp::model::{
    Annotated, CallToolResult, RawResource, Resource, ResourceContents, Tool, ToolAnnotations,
};
use rmcp::ErrorData as McpError;
use serde::Deserialize;
use serde_json::{json, Map, Value};

pub(crate) const BRIDGE_AUTH_URI: &str = "prism://bridge/auth";
pub(crate) const BRIDGE_ADOPT_TOOL_NAME: &str = "prism_bridge_adopt";

const BRIDGE_ADOPT_INPUT_SCHEMA: &str = r#"{
  "type": "object",
  "properties": {
    "profile": {
      "type": "string",
      "description": "Stored local PRISM credential profile label to bind to this bridge."
    },
    "principalId": {
      "type": "string",
      "description": "Stored principal id to bind to this bridge when no profile label is known."
    },
    "credentialId": {
      "type": "string",
      "description": "Stored credential id to bind to this bridge as an explicit fallback selector."
    }
  },
  "additionalProperties": false
}"#;

#[derive(Debug, Clone)]
pub(crate) struct BridgeBinding {
    profile: CredentialProfile,
}

impl BridgeBinding {
    pub(crate) fn profile_label(&self) -> &str {
        &self.profile.profile
    }

    pub(crate) fn principal_id(&self) -> &str {
        &self.profile.principal_id
    }

    pub(crate) fn credential_id(&self) -> &str {
        &self.profile.credential_id
    }

    pub(crate) fn credential_json(&self) -> Value {
        json!({
            "credentialId": self.profile.credential_id,
            "principalToken": self.profile.principal_token,
        })
    }
}

#[derive(Debug, Default)]
pub(crate) struct BridgeAuthState {
    binding: RwLock<Option<BridgeBinding>>,
}

impl BridgeAuthState {
    pub(crate) fn binding(&self) -> Option<BridgeBinding> {
        self.binding
            .read()
            .expect("bridge auth state lock poisoned")
            .clone()
    }

    pub(crate) fn bind(&self, binding: BridgeBinding) -> Result<BridgeBinding, McpError> {
        let mut state = self
            .binding
            .write()
            .expect("bridge auth state lock poisoned");
        if let Some(existing) = state.as_ref() {
            if existing.profile.profile == binding.profile.profile
                && existing.profile.principal_id == binding.profile.principal_id
                && existing.profile.credential_id == binding.profile.credential_id
            {
                return Ok(existing.clone());
            }
            return Err(McpError::invalid_params(
                "bridge is already bound to a different principal",
                Some(json!({
                    "code": "bridge_already_bound",
                    "boundProfile": existing.profile.profile,
                    "boundPrincipalId": existing.profile.principal_id,
                    "nextAction": "Keep using the current bridge for that principal, or start a fresh stdio bridge process for a different principal.",
                })),
            ));
        }
        *state = Some(binding.clone());
        Ok(binding)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BridgeAdoptArgs {
    pub(crate) profile: Option<String>,
    pub(crate) principal_id: Option<String>,
    pub(crate) credential_id: Option<String>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct BridgeAuthResourcePayload {
    uri: String,
    status: &'static str,
    profile: Option<String>,
    principal_id: Option<String>,
    credential_id: Option<String>,
    next_action: String,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct BridgeAdoptResult {
    status: &'static str,
    profile: String,
    principal_id: String,
    credential_id: String,
    next_action: String,
}

#[derive(Debug, Clone)]
pub(crate) struct BridgeAuthContext {
    credentials_path: Option<PathBuf>,
    state: Arc<BridgeAuthState>,
}

impl BridgeAuthContext {
    pub(crate) fn for_root(root: &Path) -> anyhow::Result<Self> {
        let credentials_path = PrismPaths::for_workspace_root(root)?.credentials_path()?;
        Ok(Self::from_credentials_path(credentials_path))
    }

    pub(crate) fn from_credentials_path(credentials_path: PathBuf) -> Self {
        Self {
            credentials_path: Some(credentials_path),
            state: Arc::new(BridgeAuthState::default()),
        }
    }

    #[cfg(test)]
    pub(crate) fn disabled() -> Self {
        Self {
            credentials_path: None,
            state: Arc::new(BridgeAuthState::default()),
        }
    }

    pub(crate) fn binding(&self) -> Option<BridgeBinding> {
        self.state.binding()
    }

    pub(crate) fn bridge_instructions_suffix(&self) -> &'static str {
        "This stdio bridge can bind itself to a locally stored PRISM principal. Before the first authoritative `prism_mutate`, call `prism_bridge_adopt` with your local profile label or principal id. After adoption, bridged `prism_mutate` calls may omit `credential` because the bridge injects the stored credential on your behalf."
    }

    pub(crate) fn bridge_auth_resource(&self) -> Resource {
        Annotated::new(
            RawResource::new(BRIDGE_AUTH_URI, "PRISM Bridge Auth")
                .with_description(
                    "Local stdio bridge principal binding status and adoption guidance.",
                )
                .with_mime_type("application/json"),
            None,
        )
    }

    pub(crate) fn bridge_auth_resource_contents(&self) -> ResourceContents {
        let payload = self.resource_payload();
        ResourceContents::text(
            serde_json::to_string_pretty(&payload).expect("bridge auth payload should serialize"),
            BRIDGE_AUTH_URI,
        )
        .with_mime_type("application/json")
    }

    pub(crate) fn bridge_adopt_tool(&self) -> Tool {
        let mut tool = Tool::default();
        tool.name = Cow::Borrowed(BRIDGE_ADOPT_TOOL_NAME);
        tool.title = Some("Adopt Bridge Principal".to_string());
        tool.description = Some(Cow::Borrowed(
            "Bind this stdio bridge to a locally stored PRISM credential profile or principal id. After adoption, bridged `prism_mutate` calls may omit `credential`.",
        ));
        tool.input_schema = Arc::new(
            serde_json::from_str(BRIDGE_ADOPT_INPUT_SCHEMA)
                .expect("bridge adopt input schema should parse"),
        );
        tool.annotations = Some(ToolAnnotations::from_raw(
            Some("Adopt Bridge Principal".to_string()),
            Some(false),
            Some(false),
            Some(true),
            None,
        ));
        tool
    }

    pub(crate) fn patch_mutation_tool(&self, mut tool: Tool) -> Tool {
        if let Some(description) = tool.description.as_mut() {
            description.to_mut().push_str(
                " When called through a bound stdio bridge, `credential` may be omitted and the bridge will inject the locally stored principal credential.",
            );
        }
        let mut schema = (*tool.input_schema).clone();
        if let Some(Value::Array(variants)) = schema.get_mut("oneOf") {
            for variant in variants {
                if let Some(required) = variant.get_mut("required").and_then(Value::as_array_mut) {
                    required.retain(|value| value.as_str() != Some("credential"));
                }
            }
        }
        tool.input_schema = Arc::new(schema);
        tool
    }

    pub(crate) fn handle_adopt(
        &self,
        arguments: Option<Map<String, Value>>,
    ) -> Result<CallToolResult, McpError> {
        let args = arguments
            .map(Value::Object)
            .unwrap_or_else(|| Value::Object(Map::new()));
        let args: BridgeAdoptArgs = serde_json::from_value(args).map_err(|error| {
            McpError::invalid_params(
                "invalid prism_bridge_adopt input",
                Some(json!({
                    "code": "bridge_adopt_invalid_input",
                    "error": error.to_string(),
                })),
            )
        })?;

        let credentials_path = self.credentials_path.as_ref().ok_or_else(|| {
            McpError::internal_error(
                "bridge-local credential adoption is unavailable for this bridge",
                Some(json!({
                    "code": "bridge_adopt_unavailable",
                    "nextAction": "Start the stdio bridge from a workspace root so it can load the local PRISM credentials store.",
                })),
            )
        })?;

        let credentials = CredentialsFile::load(credentials_path).map_err(|error| {
            McpError::internal_error(
                "failed to read the local PRISM credentials store",
                Some(json!({
                    "code": "bridge_credentials_load_failed",
                    "error": error.to_string(),
                    "credentialsPath": credentials_path.display().to_string(),
                })),
            )
        })?;
        let profile = credentials
            .find_by_selector(
                args.profile.as_deref(),
                args.principal_id.as_deref(),
                args.credential_id.as_deref(),
            )
            .map_err(|error| {
                McpError::invalid_params(
                    "no stored local PRISM credential matched the requested bridge principal selector",
                    Some(json!({
                        "code": "bridge_adopt_selector_not_found",
                        "error": error.to_string(),
                        "nextAction": "Use a stored profile label, principal id, or credential id from the local PRISM credentials store.",
                    })),
                )
            })?
            .clone();
        let binding = self.state.bind(BridgeBinding { profile })?;
        let result = BridgeAdoptResult {
            status: "bound",
            profile: binding.profile_label().to_string(),
            principal_id: binding.principal_id().to_string(),
            credential_id: binding.credential_id().to_string(),
            next_action:
                "Proceed with authoritative `prism_mutate` calls on this bridge without supplying `credential`."
                    .to_string(),
        };
        Ok(CallToolResult::structured(
            serde_json::to_value(result).expect("bridge adopt result should serialize"),
        ))
    }

    pub(crate) fn inject_mutation_credential(
        &self,
        arguments: Option<Map<String, Value>>,
    ) -> Result<Option<Map<String, Value>>, McpError> {
        let Some(mut arguments) = arguments else {
            return Ok(None);
        };
        if arguments.contains_key("credential") {
            return Ok(Some(arguments));
        }
        let binding = self.binding().ok_or_else(|| {
            McpError::invalid_params(
                "bridged prism_mutate requires a bound local principal when `credential` is omitted",
                Some(json!({
                    "code": "bridge_auth_required",
                    "nextAction": "Call `prism_bridge_adopt` with your local profile label or principal id before the first authoritative mutation on this bridge.",
                    "bridgeAuthUri": BRIDGE_AUTH_URI,
                })),
            )
        })?;
        arguments.insert("credential".to_string(), binding.credential_json());
        Ok(Some(arguments))
    }

    fn resource_payload(&self) -> BridgeAuthResourcePayload {
        match self.binding() {
            Some(binding) => BridgeAuthResourcePayload {
                uri: BRIDGE_AUTH_URI.to_string(),
                status: "bound",
                profile: Some(binding.profile_label().to_string()),
                principal_id: Some(binding.principal_id().to_string()),
                credential_id: Some(binding.credential_id().to_string()),
                next_action:
                    "Proceed with authoritative `prism_mutate` calls without supplying `credential` on this bridge."
                        .to_string(),
            },
            None => BridgeAuthResourcePayload {
                uri: BRIDGE_AUTH_URI.to_string(),
                status: "unbound",
                profile: None,
                principal_id: None,
                credential_id: None,
                next_action:
                    "Call `prism_bridge_adopt` with a local profile label or principal id before the first authoritative `prism_mutate` on this bridge."
                        .to_string(),
            },
        }
    }
}
