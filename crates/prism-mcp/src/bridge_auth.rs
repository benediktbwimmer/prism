use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use prism_core::{PrismPaths, WorktreeMode};
use rmcp::ErrorData as McpError;
use rmcp::model::{
    Annotated, CallToolResult, RawResource, Resource, ResourceContents, Tool, ToolAnnotations,
};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::BridgeIdentityView;

pub(crate) const BRIDGE_AUTH_URI: &str = "prism://bridge/auth";
pub(crate) const BRIDGE_ADOPT_TOOL_NAME: &str = "prism_bridge_adopt";

const BRIDGE_ADOPT_INPUT_SCHEMA: &str = r#"{
  "type": "object",
  "properties": {},
  "additionalProperties": false
}"#;

#[derive(Debug, Clone)]
pub(crate) struct BridgeBinding {
    worktree_id: String,
    agent_label: String,
    worktree_mode: WorktreeMode,
}

#[derive(Debug, Clone, Default)]
enum BridgeBindingState {
    #[default]
    Unbound,
    Bound(BridgeBinding),
}

impl BridgeBinding {
    pub(crate) fn worktree_id(&self) -> &str {
        &self.worktree_id
    }

    pub(crate) fn agent_label(&self) -> &str {
        &self.agent_label
    }

    pub(crate) fn worktree_mode(&self) -> WorktreeMode {
        self.worktree_mode
    }

    pub(crate) fn bridge_execution_json(&self) -> Value {
        json!({
            "worktreeId": self.worktree_id,
            "agentLabel": self.agent_label,
        })
    }
}

#[derive(Debug, Default)]
pub(crate) struct BridgeAuthState {
    binding: RwLock<BridgeBindingState>,
}

impl BridgeAuthState {
    fn snapshot(&self) -> BridgeBindingState {
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
        if let BridgeBindingState::Bound(existing) = &*state {
            if existing.worktree_id == binding.worktree_id {
                return Ok(existing.clone());
            }
            return Err(McpError::invalid_params(
                "bridge is already attached to a different worktree execution lane",
                Some(json!({
                    "code": "bridge_already_bound",
                    "boundWorktreeId": existing.worktree_id,
                    "boundAgentLabel": existing.agent_label,
                    "nextAction": "Keep using the current bridge for that worktree, or start a fresh stdio bridge process for a different worktree.",
                })),
            ));
        }
        *state = BridgeBindingState::Bound(binding.clone());
        Ok(binding)
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BridgeAdoptArgs {}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct BridgeAuthResourcePayload {
    uri: String,
    status: String,
    worktree_id: Option<String>,
    agent_label: Option<String>,
    worktree_mode: Option<String>,
    error: Option<String>,
    next_action: String,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct BridgeAdoptResult {
    status: &'static str,
    worktree_id: String,
    agent_label: String,
    worktree_mode: String,
    next_action: String,
}

#[derive(Debug, Clone)]
pub(crate) struct BridgeAuthContext {
    root: Option<PathBuf>,
    state: Arc<BridgeAuthState>,
}

impl BridgeAuthContext {
    pub(crate) fn for_root(root: &Path) -> anyhow::Result<Self> {
        PrismPaths::for_workspace_root(root)?;
        Ok(Self::from_root(root.to_path_buf()))
    }

    pub(crate) fn from_root(root: PathBuf) -> Self {
        Self {
            root: Some(root),
            state: Arc::new(BridgeAuthState::default()),
        }
    }

    #[cfg(test)]
    pub(crate) fn disabled() -> Self {
        Self {
            root: None,
            state: Arc::new(BridgeAuthState::default()),
        }
    }

    pub(crate) fn bridge_instructions_suffix(&self) -> &'static str {
        "This stdio bridge can attach itself to the registered agent worktree for the current repository. Before the first authoritative `prism_code` mutation call, invoke `prism_bridge_adopt`. After adoption, bridged `prism_code` calls may omit `credential` because the bridge injects its attached worktree execution binding on your behalf."
    }

    pub(crate) fn bridge_auth_resource(&self) -> Resource {
        Annotated::new(
            RawResource::new(BRIDGE_AUTH_URI, "PRISM Bridge Auth")
                .with_description(
                    "Local stdio bridge worktree attachment status and adoption guidance.",
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
        tool.title = Some("Attach Bridge Worktree".to_string());
        tool.description = Some(Cow::Borrowed(
            "Attach this stdio bridge to the registered local agent worktree for authoritative mutations. After attachment, bridged `prism_code` calls may omit `credential`.",
        ));
        tool.input_schema = Arc::new(
            serde_json::from_str(BRIDGE_ADOPT_INPUT_SCHEMA)
                .expect("bridge adopt input schema should parse"),
        );
        tool.annotations = Some(ToolAnnotations::from_raw(
            Some("Attach Bridge Worktree".to_string()),
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
                " When called through an attached stdio bridge, `credential` may be omitted and the bridge will inject its local worktree execution binding.",
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
        let _args: BridgeAdoptArgs = serde_json::from_value(args).map_err(|error| {
            McpError::invalid_params(
                "invalid prism_bridge_adopt input",
                Some(json!({
                    "code": "bridge_adopt_invalid_input",
                    "error": error.to_string(),
                })),
            )
        })?;

        let root = self.root.as_ref().ok_or_else(|| {
            McpError::internal_error(
                "bridge-local worktree attachment is unavailable for this bridge",
                Some(json!({
                    "code": "bridge_adopt_unavailable",
                    "nextAction": "Start the stdio bridge from a workspace root so it can attach to the local registered worktree.",
                })),
            )
        })?;
        let paths = PrismPaths::for_workspace_root(root).map_err(|error| {
            McpError::internal_error(
                "failed to resolve the local PRISM worktree paths",
                Some(json!({
                    "code": "bridge_worktree_paths_failed",
                    "error": error.to_string(),
                })),
            )
        })?;
        let registration = paths.worktree_registration().map_err(|error| {
            McpError::internal_error(
                "failed to read local PRISM worktree registration metadata",
                Some(json!({
                    "code": "bridge_worktree_registration_load_failed",
                    "error": error.to_string(),
                })),
            )
        })?;
        let registration = registration.ok_or_else(|| {
            McpError::invalid_params(
                "the current worktree is not registered for authoritative PRISM mutations",
                Some(json!({
                    "code": "bridge_worktree_unregistered",
                    "nextAction": "Register this worktree as an agent worktree before calling `prism_bridge_adopt`.",
                })),
            )
        })?;
        if registration.mode != WorktreeMode::Agent {
            return Err(McpError::invalid_params(
                "the current worktree is not registered as an agent execution lane",
                Some(json!({
                    "code": "bridge_worktree_mode_not_agent",
                    "worktreeId": registration.worktree_id,
                    "agentLabel": registration.agent_label,
                    "worktreeMode": "human",
                    "nextAction": "Use a worktree registered in `agent` mode for bridge-based authoritative mutations.",
                })),
            ));
        }
        let binding = self.state.bind(BridgeBinding {
            worktree_id: registration.worktree_id.clone(),
            agent_label: registration.agent_label.clone(),
            worktree_mode: registration.mode,
        })?;
        let result = BridgeAdoptResult {
            status: "bound",
            worktree_id: binding.worktree_id().to_string(),
            agent_label: binding.agent_label().to_string(),
            worktree_mode: "agent".to_string(),
            next_action:
                "Proceed with authoritative `prism_code` calls on this bridge without supplying `credential`."
                    .to_string(),
        };
        Ok(CallToolResult::structured(
            serde_json::to_value(result).expect("bridge adopt result should serialize"),
        ))
    }

    pub(crate) fn inject_mutation_bridge_execution(
        &self,
        arguments: Option<Map<String, Value>>,
    ) -> Result<Option<Map<String, Value>>, McpError> {
        let Some(mut arguments) = arguments else {
            return Ok(None);
        };
        if arguments.contains_key("credential") || arguments.contains_key("bridgeExecution") {
            return Ok(Some(arguments));
        }
        let binding = match self.state.snapshot() {
            BridgeBindingState::Bound(binding) => binding,
            BridgeBindingState::Unbound => {
                return Err(McpError::invalid_params(
                    "bridged prism_code requires an attached local agent worktree when `credential` is omitted",
                    Some(json!({
                        "code": "bridge_auth_required",
                        "nextAction": "Call `prism_bridge_adopt` to attach this bridge to the registered local agent worktree before the first authoritative prism_code mutation.",
                        "bridgeAuthUri": BRIDGE_AUTH_URI,
                    })),
                ));
            }
        };
        arguments.insert(
            "bridgeExecution".to_string(),
            binding.bridge_execution_json(),
        );
        Ok(Some(arguments))
    }

    pub(crate) fn session_bridge_identity(&self) -> BridgeIdentityView {
        match self.state.snapshot() {
            BridgeBindingState::Bound(binding) => BridgeIdentityView {
                status: "bound".to_string(),
                profile: None,
                principal_id: None,
                credential_id: None,
                worktree_id: Some(binding.worktree_id.clone()),
                agent_label: Some(binding.agent_label.clone()),
                worktree_mode: Some(match binding.worktree_mode() {
                    WorktreeMode::Human => "human".to_string(),
                    WorktreeMode::Agent => "agent".to_string(),
                }),
                error: None,
                next_action:
                    "Proceed with authoritative `prism_mutate` calls without supplying `credential` on this bridge."
                        .to_string(),
            },
            BridgeBindingState::Unbound => BridgeIdentityView {
                status: "unbound".to_string(),
                profile: None,
                principal_id: None,
                credential_id: None,
                worktree_id: None,
                agent_label: None,
                worktree_mode: None,
                error: None,
                next_action:
                    "Call `prism_bridge_adopt` to attach this bridge to the registered local agent worktree before the first authoritative `prism_mutate`."
                        .to_string(),
            },
        }
    }

    fn resource_payload(&self) -> BridgeAuthResourcePayload {
        let identity = self.session_bridge_identity();
        BridgeAuthResourcePayload {
            uri: BRIDGE_AUTH_URI.to_string(),
            status: identity.status,
            worktree_id: identity.worktree_id,
            agent_label: identity.agent_label,
            worktree_mode: identity.worktree_mode,
            error: identity.error,
            next_action: identity.next_action,
        }
    }
}
