use prism_core::{
    AdmissionBusyError, AuthenticatedPrincipal, ObservedChangeFlushTrigger, PrismPaths,
    WorktreeMode, WorktreeMutatorSlotError,
};
use prism_ir::{CredentialCapability, CredentialId, PrincipalKind};
use prism_js::{
    AgentConceptResultView, AgentExpandResultView, AgentGatherResultView, AgentLocateResultView,
    AgentOpenResultView, AgentWorksetResultView, QueryPhaseView,
};
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    service::RequestContext,
    tool, tool_router, ErrorData as McpError, RoleServer, ServerHandler,
};
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::mutation_trace::MutationRun;
use crate::*;

pub(crate) struct MutationOutcomeMeta {
    task_id: Option<String>,
    result_ids: Vec<String>,
    violation_count: usize,
}

#[derive(Clone, Copy)]
pub(crate) enum MutationRefreshPolicy {
    None,
    #[allow(dead_code)]
    PersistedOnly,
}

#[derive(Clone, Copy)]
enum MutationCapabilityRequirement {
    AnyAuthenticated,
    MutateCoordination,
    MutateRepoMemory,
}

impl MutationCapabilityRequirement {
    fn label(self) -> &'static str {
        match self {
            Self::AnyAuthenticated => "authenticated_principal",
            Self::MutateCoordination => "mutate_coordination",
            Self::MutateRepoMemory => "mutate_repo_memory",
        }
    }

    fn allows(self, authenticated: &AuthenticatedPrincipal) -> bool {
        matches!(self, Self::AnyAuthenticated)
            || authenticated
                .credential
                .capabilities
                .contains(&CredentialCapability::All)
            || authenticated.credential.capabilities.contains(&match self {
                Self::AnyAuthenticated => return true,
                Self::MutateCoordination => CredentialCapability::MutateCoordination,
                Self::MutateRepoMemory => CredentialCapability::MutateRepoMemory,
            })
    }
}

enum MutationAuthentication {
    Principal(AuthenticatedPrincipal),
    WorktreeExecutor,
}

impl MutationAuthentication {
    fn authenticated_principal(&self) -> Option<&AuthenticatedPrincipal> {
        match self {
            Self::Principal(authenticated) => Some(authenticated),
            Self::WorktreeExecutor => None,
        }
    }
}

impl MutationOutcomeMeta {
    pub(crate) fn task(
        task_id: Option<String>,
        result_ids: Vec<String>,
        violation_count: usize,
    ) -> Self {
        Self {
            task_id,
            result_ids,
            violation_count,
        }
    }

    pub(crate) fn coordination(result_ids: Vec<String>, violation_count: usize) -> Self {
        Self {
            task_id: None,
            result_ids,
            violation_count,
        }
    }
}

impl PrismMcpServer {
    fn worktree_mode_label(mode: WorktreeMode) -> &'static str {
        match mode {
            WorktreeMode::Human => "human",
            WorktreeMode::Agent => "agent",
        }
    }

    fn required_worktree_mode_for_principal(kind: PrincipalKind) -> WorktreeMode {
        match kind {
            PrincipalKind::Human => WorktreeMode::Human,
            PrincipalKind::Agent
            | PrincipalKind::Service
            | PrincipalKind::System
            | PrincipalKind::Ci
            | PrincipalKind::External => WorktreeMode::Agent,
        }
    }

    fn load_registered_worktree_for_authenticated_mutation(
        &self,
        workspace: &prism_core::WorkspaceSession,
        authenticated: &AuthenticatedPrincipal,
    ) -> Result<prism_core::WorktreeRegistrationRecord, McpError> {
        let paths = PrismPaths::for_workspace_root(workspace.root()).map_err(|error| {
            McpError::internal_error(
                "failed to resolve the current PRISM worktree paths",
                Some(json!({
                    "code": "mutation_worktree_paths_failed",
                    "error": error.to_string(),
                })),
            )
        })?;
        let registration = paths.worktree_registration().map_err(|error| {
            McpError::internal_error(
                "failed to read the current PRISM worktree registration",
                Some(json!({
                    "code": "mutation_worktree_registration_load_failed",
                    "error": error.to_string(),
                })),
            )
        })?;
        let registration = registration.ok_or_else(|| {
            McpError::invalid_params(
                "authoritative mutations require a registered worktree",
                Some(json!({
                    "code": "mutation_worktree_unregistered",
                    "principalId": authenticated.principal.principal_id.0,
                    "principalKind": authenticated.principal.kind,
                    "nextAction": "Register this worktree in the correct mode before retrying the mutation.",
                })),
            )
        })?;
        let required_mode =
            Self::required_worktree_mode_for_principal(authenticated.principal.kind);
        if registration.mode != required_mode {
            return Err(McpError::invalid_params(
                "principal kind does not match the current registered worktree mode",
                Some(json!({
                    "code": "mutation_worktree_mode_mismatch",
                    "principalId": authenticated.principal.principal_id.0,
                    "principalKind": authenticated.principal.kind,
                    "worktreeId": registration.worktree_id,
                    "agentLabel": registration.agent_label,
                    "worktreeMode": Self::worktree_mode_label(registration.mode),
                    "requiredWorktreeMode": Self::worktree_mode_label(required_mode),
                    "nextAction": "Use a worktree registered in the matching mode for this actor, or retry through the appropriate bridge or human session.",
                })),
            ));
        }
        Ok(registration)
    }

    fn map_worktree_mutator_slot_error(error: WorktreeMutatorSlotError) -> McpError {
        match error {
            WorktreeMutatorSlotError::Conflict(conflict) => McpError::invalid_params(
                "prism_mutate conflicts with the active worktree mutator session",
                Some(json!({
                    "code": "mutation_worktree_mutator_slot_conflict",
                    "worktreeId": conflict.worktree_id,
                    "currentOwner": {
                        "sessionId": conflict.current_owner.session_id,
                        "authorityId": conflict.current_owner.authority_id,
                        "principalId": conflict.current_owner.principal_id,
                        "name": conflict.current_owner.principal_name,
                        "credentialId": conflict.current_owner.credential_id,
                        "lastHeartbeatAt": conflict.current_owner.last_heartbeat_at,
                    },
                    "attemptedSessionId": conflict.attempted_session_id,
                    "attemptedPrincipal": {
                        "authorityId": conflict.attempted_principal.authority_id,
                        "principalId": conflict.attempted_principal.principal_id,
                        "name": conflict.attempted_principal.principal_name,
                    },
                    "staleAt": conflict.stale_at,
                    "nextAction": "Retry after the current worktree mutator session goes stale, or have a human operator explicitly take over the worktree before retrying authenticated mutations.",
                })),
            ),
            WorktreeMutatorSlotError::TakeoverRequiresHuman {
                principal_id,
                principal_kind,
            } => McpError::invalid_params(
                "only a human principal can authorize worktree mutator takeover",
                Some(json!({
                    "code": "mutation_worktree_mutator_takeover_requires_human",
                    "principalId": principal_id,
                    "principalKind": principal_kind,
                    "nextAction": "Retry with an authenticated human operator session if this worktree really needs an explicit takeover.",
                })),
            ),
            WorktreeMutatorSlotError::Storage(error) => McpError::internal_error(
                "failed to update the worktree mutator slot",
                Some(json!({
                    "code": "mutation_worktree_mutator_slot_storage_failed",
                    "error": error.to_string(),
                })),
            ),
        }
    }

    fn payload_has_nonempty_string(payload: &Value, keys: &[&str]) -> bool {
        keys.iter().any(|key| {
            payload
                .get(*key)
                .and_then(Value::as_str)
                .is_some_and(|value| !value.trim().is_empty())
        })
    }

    fn coordination_has_explicit_work_subject(args: &PrismCoordinationArgs) -> bool {
        if args.task_id.is_some() {
            return true;
        }
        match args.kind {
            CoordinationMutationKindInput::PlanBootstrap
            | CoordinationMutationKindInput::PlanCreate => false,
            CoordinationMutationKindInput::PlanUpdate
            | CoordinationMutationKindInput::PlanArchive => {
                Self::payload_has_nonempty_string(&args.payload, &["planId", "plan_id"])
            }
            CoordinationMutationKindInput::TaskCreate => {
                Self::payload_has_nonempty_string(&args.payload, &["planId", "plan_id"])
            }
            CoordinationMutationKindInput::Update => {
                Self::payload_has_nonempty_string(&args.payload, &["id", "taskId", "task_id"])
            }
            CoordinationMutationKindInput::Handoff
            | CoordinationMutationKindInput::Resume
            | CoordinationMutationKindInput::Reclaim
            | CoordinationMutationKindInput::HandoffAccept => {
                Self::payload_has_nonempty_string(&args.payload, &["taskId", "task_id"])
            }
        }
    }

    fn resource_content_uri(content: &ResourceContents) -> &str {
        match content {
            ResourceContents::TextResourceContents { uri, .. }
            | ResourceContents::BlobResourceContents { uri, .. } => uri.as_str(),
        }
    }

    fn record_surface_call(
        &self,
        call_type: &str,
        name: &str,
        summary: String,
        started_at: u64,
        started: Instant,
        success: bool,
        error: Option<String>,
        request_preview: Option<serde_json::Value>,
        response_preview: Option<serde_json::Value>,
        metadata: serde_json::Value,
        extra_phases: Vec<QueryPhaseView>,
    ) {
        let mut duration_ms = crate::mcp_call_log::duration_to_ms(started.elapsed());
        let mut started_at = started_at;
        let touched = request_preview
            .as_ref()
            .map(crate::mcp_call_log::touches_for_value)
            .unwrap_or_default();
        let request_args = json!({ "callType": call_type, "name": name });
        let mut phases = extra_phases;
        phases.push(QueryPhaseView {
            operation: "mcp.executeHandler".to_string(),
            started_at,
            duration_ms,
            args_summary: Some(crate::mcp_call_log::summarize_value(&request_args)),
            touched: touched.clone(),
            success,
            error: error.clone(),
        });
        phases.push(QueryPhaseView {
            operation: format!("{call_type}.{name}"),
            started_at,
            duration_ms,
            args_summary: request_preview
                .as_ref()
                .map(crate::mcp_call_log::summarize_value),
            touched: touched.clone(),
            success,
            error: error.clone(),
        });
        if success {
            phases.push(QueryPhaseView {
                operation: "mcp.encodeResponse".to_string(),
                started_at,
                duration_ms: 0,
                args_summary: Some(crate::mcp_call_log::summarize_value(&request_args)),
                touched: Vec::new(),
                success: true,
                error: None,
            });
        }
        let mut metadata = metadata;
        crate::request_envelope::apply_current_request_envelope(
            &mut phases,
            &mut started_at,
            &mut duration_ms,
            &mut metadata,
        );
        crate::slow_call_snapshot::attach_slow_call_snapshot(
            &mut metadata,
            duration_ms,
            self.host.workspace_session().map(Arc::as_ref),
        );
        let entry = crate::mcp_call_log::new_log_entry(
            self.host.mcp_call_log_store.runtime(),
            call_type,
            name,
            None,
            summary,
            started_at,
            duration_ms,
            Some(self.session.session_id().0.to_string()),
            self.session
                .effective_current_task()
                .map(|task| task.0.to_string()),
            success,
            error,
            crate::mcp_call_log::unique_operations(&phases),
            crate::mcp_call_log::unique_touches(&phases),
            Vec::new(),
            crate::mcp_call_log::payload_summary(request_preview.as_ref()),
            crate::mcp_call_log::payload_summary(response_preview.as_ref()),
        );
        let _ = self.host.mcp_call_log_store.push(PersistedMcpCallRecord {
            entry,
            phases,
            request_payload: request_preview.clone(),
            request_preview: request_preview
                .as_ref()
                .and_then(crate::mcp_call_log::preview_value),
            response_preview: response_preview
                .as_ref()
                .and_then(crate::mcp_call_log::preview_value),
            metadata,
            query_compat: None,
            mutation_compat: None,
        });
    }

    pub(crate) fn build_tool_router() -> ToolRouter<Self> {
        let _: fn(&Self, Parameters<PrismConceptArgs>) -> Result<CallToolResult, McpError> =
            Self::prism_concept;
        let _: fn(&Self, Parameters<PrismTaskBriefArgs>) -> Result<CallToolResult, McpError> =
            Self::prism_task_brief;
        Self::tool_router()
    }

    pub(crate) fn execute_prism_mutation_via_tool(
        &self,
        args: PrismMutationArgs,
    ) -> Result<PrismMutationResult, McpError> {
        let response = self.prism_mutate(Parameters(args))?;
        let structured = response.structured_content.ok_or_else(|| {
            McpError::internal_error(
                "prism_mutate did not return structured content",
                Some(json!({
                    "code": "mutation_missing_structured_content",
                })),
            )
        })?;
        serde_json::from_value::<PrismMutationResult>(structured).map_err(|error| {
            McpError::internal_error(
                "failed to decode structured prism_mutate result",
                Some(json!({
                    "code": "mutation_result_decode_failed",
                    "error": error.to_string(),
                })),
            )
        })
    }

    pub(crate) fn transport_bind_tool_schema(mut tool: Tool, features: &PrismMcpFeatures) -> Tool {
        if let Some(Value::Object(schema)) =
            tool_transport_input_schema_value_with_features(tool.name.as_ref(), features)
        {
            tool.input_schema = Arc::new(schema);
        }
        tool
    }

    fn authenticate_mutation(
        &self,
        credential: Option<&PrismMutationCredentialArgs>,
        bridge_execution: Option<&PrismMutationBridgeExecutionArgs>,
        requirement: MutationCapabilityRequirement,
    ) -> Result<MutationAuthentication, McpError> {
        if let Some(credential) = credential {
            return self.authenticate_principal_mutation(credential, requirement);
        }
        if let Some(bridge_execution) = bridge_execution {
            self.authenticate_bridge_execution(bridge_execution)?;
            return Ok(MutationAuthentication::WorktreeExecutor);
        }
        Err(McpError::invalid_params(
            "prism_mutate requires either `credential` or an attached bridge execution binding",
            Some(json!({
                "code": "mutation_auth_missing",
                "nextAction": "Supply `credential` directly, or call `prism_bridge_adopt` on a stdio bridge attached to a registered agent worktree before retrying the mutation.",
            })),
        ))
    }

    fn authenticate_principal_mutation(
        &self,
        credential: &PrismMutationCredentialArgs,
        requirement: MutationCapabilityRequirement,
    ) -> Result<MutationAuthentication, McpError> {
        let workspace = self.host.workspace_session().ok_or_else(|| {
            McpError::internal_error(
                "prism_mutate requires a workspace-backed session",
                Some(json!({
                    "code": "mutation_auth_workspace_required",
                    "nextAction": "Run the mutation against a workspace-backed PRISM MCP session so the server can verify principal credentials.",
                })),
            )
        })?;
        let authenticated = workspace
            .authenticate_principal_credential(
                &CredentialId::new(credential.credential_id.clone()),
                &credential.principal_token,
            )
            .map_err(|error| {
                McpError::invalid_params(
                    "prism_mutate credential rejected",
                    Some(json!({
                        "code": "mutation_auth_failed",
                        "credentialId": credential.credential_id,
                        "error": error.to_string(),
                        "nextAction": "Use `prism auth login` or mint a fresh credential, then retry the mutation.",
                    })),
                )
            })?;
        self.load_registered_worktree_for_authenticated_mutation(
            workspace.as_ref(),
            &authenticated,
        )?;
        workspace
            .acquire_or_refresh_worktree_mutator_slot(&authenticated, &self.session.session_id())
            .map_err(Self::map_worktree_mutator_slot_error)?;
        if !requirement.allows(&authenticated) {
            return Err(McpError::invalid_params(
                "prism_mutate credential lacks the required capability",
                Some(json!({
                    "code": "mutation_capability_denied",
                    "requiredCapability": requirement.label(),
                    "credentialId": authenticated.credential.credential_id.0,
                    "principalId": authenticated.principal.principal_id.0,
                    "nextAction": "Use a credential with the required capability or mint a new child principal with narrower capabilities for this mutation lane.",
                })),
            ));
        }
        Ok(MutationAuthentication::Principal(authenticated))
    }

    fn authenticate_bridge_execution(
        &self,
        bridge_execution: &PrismMutationBridgeExecutionArgs,
    ) -> Result<(), McpError> {
        let workspace = self.host.workspace_session().ok_or_else(|| {
            McpError::internal_error(
                "prism_mutate requires a workspace-backed session",
                Some(json!({
                    "code": "mutation_auth_workspace_required",
                    "nextAction": "Run the mutation against a workspace-backed PRISM MCP session so the server can verify the attached worktree execution lane.",
                })),
            )
        })?;
        let paths = PrismPaths::for_workspace_root(workspace.root()).map_err(|error| {
            McpError::internal_error(
                "failed to resolve the current PRISM worktree paths",
                Some(json!({
                    "code": "mutation_worktree_paths_failed",
                    "error": error.to_string(),
                })),
            )
        })?;
        let registration = paths.worktree_registration().map_err(|error| {
            McpError::internal_error(
                "failed to read the current PRISM worktree registration",
                Some(json!({
                    "code": "mutation_worktree_registration_load_failed",
                    "error": error.to_string(),
                })),
            )
        })?;
        let registration = registration.ok_or_else(|| {
            McpError::invalid_params(
                "authoritative mutations require a registered worktree",
                Some(json!({
                    "code": "mutation_worktree_unregistered",
                    "nextAction": "Register this worktree before retrying the mutation, or supply an explicit human/service credential.",
                })),
            )
        })?;
        if registration.mode != WorktreeMode::Agent {
            return Err(McpError::invalid_params(
                "bridge execution requires a worktree registered in agent mode",
                Some(json!({
                    "code": "mutation_bridge_execution_requires_agent_worktree",
                    "worktreeId": registration.worktree_id,
                    "agentLabel": registration.agent_label,
                    "worktreeMode": "human",
                    "nextAction": "Use a bridge attached to a registered agent worktree, or supply an explicit human credential for direct operator mutation.",
                })),
            ));
        }
        if registration.worktree_id != bridge_execution.worktree_id
            || registration.agent_label != bridge_execution.agent_label
        {
            return Err(McpError::invalid_params(
                "bridge execution binding does not match the current registered worktree",
                Some(json!({
                    "code": "mutation_bridge_execution_mismatch",
                    "expectedWorktreeId": registration.worktree_id,
                    "expectedAgentLabel": registration.agent_label,
                    "receivedWorktreeId": bridge_execution.worktree_id,
                    "receivedAgentLabel": bridge_execution.agent_label,
                    "nextAction": "Call `prism_bridge_adopt` again so the bridge reattaches to the current registered worktree lane.",
                })),
            ));
        }
        workspace
            .acquire_or_refresh_agent_worktree_mutator_slot(&self.session.session_id())
            .map_err(Self::map_worktree_mutator_slot_error)?;
        Ok(())
    }

    fn authenticate_mutation_with_run(
        &self,
        run: &MutationRun,
        credential: Option<&PrismMutationCredentialArgs>,
        bridge_execution: Option<&PrismMutationBridgeExecutionArgs>,
        requirement: MutationCapabilityRequirement,
    ) -> Result<MutationAuthentication, McpError> {
        if let Some(credential) = credential {
            return self.authenticate_principal_mutation_with_run(run, credential, requirement);
        }
        if let Some(bridge_execution) = bridge_execution {
            let started = Instant::now();
            let result = self.authenticate_bridge_execution(bridge_execution);
            run.record_phase(
                "mutation.auth.bridgeExecution",
                &json!({
                    "worktreeId": bridge_execution.worktree_id,
                    "agentLabel": bridge_execution.agent_label,
                }),
                started.elapsed(),
                result.is_ok(),
                result.as_ref().err().map(ToString::to_string),
            );
            return result.map(|_| MutationAuthentication::WorktreeExecutor);
        }
        Err(McpError::invalid_params(
            "prism_mutate requires either `credential` or an attached bridge execution binding",
            Some(json!({
                "code": "mutation_auth_missing",
                "nextAction": "Supply `credential` directly, or call `prism_bridge_adopt` on a stdio bridge attached to a registered agent worktree before retrying the mutation.",
            })),
        ))
    }

    fn authenticate_principal_mutation_with_run(
        &self,
        run: &MutationRun,
        credential: &PrismMutationCredentialArgs,
        requirement: MutationCapabilityRequirement,
    ) -> Result<MutationAuthentication, McpError> {
        let total_started = Instant::now();
        let workspace_started = Instant::now();
        let workspace = self.host.workspace_session().ok_or_else(|| {
            McpError::internal_error(
                "prism_mutate requires a workspace-backed session",
                Some(json!({
                    "code": "mutation_auth_workspace_required",
                    "nextAction": "Run the mutation against a workspace-backed PRISM MCP session so the server can verify principal credentials.",
                })),
            )
        });
        match workspace {
            Ok(workspace) => {
                run.record_phase(
                    "mutation.auth.workspaceSession",
                    &json!({ "required": true }),
                    workspace_started.elapsed(),
                    true,
                    None,
                );
                let verify_started = Instant::now();
                let authenticated = workspace
                    .authenticate_principal_credential(
                        &CredentialId::new(credential.credential_id.clone()),
                        &credential.principal_token,
                    )
                    .map_err(|error| {
                        McpError::invalid_params(
                            "prism_mutate credential rejected",
                            Some(json!({
                                "code": "mutation_auth_failed",
                                "credentialId": credential.credential_id,
                                "error": error.to_string(),
                                "nextAction": "Use `prism auth login` or mint a fresh credential, then retry the mutation.",
                            })),
                        )
                    });
                let authenticated = match authenticated {
                    Ok(authenticated) => {
                        run.record_phase(
                            "mutation.auth.verifyCredential",
                            &json!({ "credentialId": credential.credential_id }),
                            verify_started.elapsed(),
                            true,
                            None,
                        );
                        authenticated
                    }
                    Err(error) => {
                        run.record_phase(
                            "mutation.auth.verifyCredential",
                            &json!({ "credentialId": credential.credential_id }),
                            verify_started.elapsed(),
                            false,
                            Some(error.to_string()),
                        );
                        return Err(error);
                    }
                };
                let registration_started = Instant::now();
                let registration_result = self.load_registered_worktree_for_authenticated_mutation(
                    workspace.as_ref(),
                    &authenticated,
                );
                let registration = match registration_result {
                    Ok(registration) => {
                        run.record_phase(
                            "mutation.auth.requireRegisteredWorktree",
                            &json!({
                                "principalKind": authenticated.principal.kind,
                                "worktreeId": registration.worktree_id,
                                "worktreeMode": Self::worktree_mode_label(registration.mode),
                            }),
                            registration_started.elapsed(),
                            true,
                            None,
                        );
                        registration
                    }
                    Err(error) => {
                        run.record_phase(
                            "mutation.auth.requireRegisteredWorktree",
                            &json!({
                                "principalKind": authenticated.principal.kind,
                            }),
                            registration_started.elapsed(),
                            false,
                            Some(error.to_string()),
                        );
                        return Err(error);
                    }
                };
                let bind_started = Instant::now();
                if let Err(error) = workspace.acquire_or_refresh_worktree_mutator_slot(
                    &authenticated,
                    &self.session.session_id(),
                ) {
                    let mapped = Self::map_worktree_mutator_slot_error(error);
                    run.record_phase(
                        "mutation.auth.acquireWorktreeMutatorSlot",
                        &json!({ "credentialId": credential.credential_id }),
                        bind_started.elapsed(),
                        false,
                        Some(mapped.to_string()),
                    );
                    return Err(mapped);
                }
                run.record_phase(
                    "mutation.auth.acquireWorktreeMutatorSlot",
                    &json!({
                        "credentialId": credential.credential_id,
                        "worktreeId": registration.worktree_id,
                        "worktreeMode": Self::worktree_mode_label(registration.mode),
                    }),
                    bind_started.elapsed(),
                    true,
                    None,
                );
                let capability_started = Instant::now();
                if !requirement.allows(&authenticated) {
                    let mapped = McpError::invalid_params(
                        "prism_mutate credential lacks the required capability",
                        Some(json!({
                            "code": "mutation_capability_denied",
                            "requiredCapability": requirement.label(),
                            "credentialId": authenticated.credential.credential_id.0,
                            "principalId": authenticated.principal.principal_id.0,
                            "nextAction": "Use a credential with the required capability or mint a new child principal with narrower capabilities for this mutation lane.",
                        })),
                    );
                    run.record_phase(
                        "mutation.auth.checkCapability",
                        &json!({ "requiredCapability": requirement.label() }),
                        capability_started.elapsed(),
                        false,
                        Some(mapped.to_string()),
                    );
                    return Err(mapped);
                }
                run.record_phase(
                    "mutation.auth.checkCapability",
                    &json!({ "requiredCapability": requirement.label() }),
                    capability_started.elapsed(),
                    true,
                    None,
                );
                run.record_phase(
                    "mutation.auth",
                    &json!({
                        "credentialId": credential.credential_id,
                        "requiredCapability": requirement.label(),
                    }),
                    total_started.elapsed(),
                    true,
                    None,
                );
                Ok(MutationAuthentication::Principal(authenticated))
            }
            Err(error) => {
                run.record_phase(
                    "mutation.auth.workspaceSession",
                    &json!({ "required": true }),
                    workspace_started.elapsed(),
                    false,
                    Some(error.to_string()),
                );
                Err(error)
            }
        }
    }

    fn require_declared_work_context(
        &self,
        action_name: &'static str,
        has_explicit_work_subject: bool,
    ) -> Result<(), McpError> {
        if has_explicit_work_subject || self.session.current_work_state().is_some() {
            return Ok(());
        }
        Err(McpError::invalid_params(
            "declared work context required before mutation",
            Some(json!({
                "code": "mutation_declared_work_required",
                "action": action_name,
                "nextAction": "Call `prism_mutate` with `action: \"declare_work\"` to declare intent before retrying this mutation, or provide an explicit taskId/claimId when the action supports it.",
            })),
        ))
    }

    fn require_declared_work_context_with_run(
        &self,
        run: &MutationRun,
        action_name: &'static str,
        has_explicit_work_subject: bool,
    ) -> Result<(), McpError> {
        let started = Instant::now();
        let result = self.require_declared_work_context(action_name, has_explicit_work_subject);
        run.record_phase(
            "mutation.requireDeclaredWork",
            &json!({
                "action": action_name,
                "hasExplicitWorkSubject": has_explicit_work_subject,
            }),
            started.elapsed(),
            result.is_ok(),
            result.as_ref().err().map(ToString::to_string),
        );
        result
    }

    pub(crate) fn execute_logged_mutation<T, F, G>(
        &self,
        action: &str,
        refresh_policy: MutationRefreshPolicy,
        operation: F,
        finish: G,
    ) -> Result<T, McpError>
    where
        T: Serialize,
        F: FnOnce() -> Result<T, anyhow::Error>,
        G: FnOnce(&T) -> MutationOutcomeMeta,
    {
        self.execute_logged_mutation_with_run(action, refresh_policy, |_| operation(), finish)
    }

    pub(crate) fn execute_logged_mutation_with_run<T, F, G>(
        &self,
        action: &str,
        refresh_policy: MutationRefreshPolicy,
        operation: F,
        finish: G,
    ) -> Result<T, McpError>
    where
        T: Serialize,
        F: FnOnce(&MutationRun) -> Result<T, anyhow::Error>,
        G: FnOnce(&T) -> MutationOutcomeMeta,
    {
        let run = self.host.begin_mutation_run(self.session.as_ref(), action);
        self.execute_logged_mutation_with_existing_run(
            run,
            action,
            refresh_policy,
            operation,
            finish,
        )
    }

    fn execute_logged_mutation_with_existing_run<T, F, G>(
        &self,
        run: MutationRun,
        action: &str,
        refresh_policy: MutationRefreshPolicy,
        operation: F,
        finish: G,
    ) -> Result<T, McpError>
    where
        T: Serialize,
        F: FnOnce(&MutationRun) -> Result<T, anyhow::Error>,
        G: FnOnce(&T) -> MutationOutcomeMeta,
    {
        let refresh_started = Instant::now();
        match refresh_policy {
            MutationRefreshPolicy::None => run.record_phase(
                "mutation.refreshWorkspace",
                &json!({ "refreshPath": "skipped" }),
                refresh_started.elapsed(),
                true,
                None,
            ),
            MutationRefreshPolicy::PersistedOnly => {
                match self.host.refresh_workspace_for_mutation() {
                    Ok(refresh) => {
                        crate::refresh_phases::record_mutation_runtime_sync_phases(&run, &refresh);
                        run.record_phase(
                            "mutation.refreshWorkspace",
                            &json!({
                                "refreshPath": refresh.refresh_path,
                                "deferred": refresh.deferred,
                                "episodicReloaded": refresh.episodic_reloaded,
                                "inferenceReloaded": refresh.inference_reloaded,
                                "coordinationReloaded": refresh.coordination_reloaded,
                                "metrics": refresh.metrics.as_json(),
                            }),
                            refresh_started.elapsed(),
                            true,
                            None,
                        )
                    }
                    Err(error) => {
                        let message = error.to_string();
                        let args = error
                            .downcast_ref::<AdmissionBusyError>()
                            .map(|busy| {
                                json!({
                                    "refreshPath": "busy",
                                    "operation": busy.operation(),
                                    "resource": busy.resource(),
                                    "retryable": true,
                                })
                            })
                            .unwrap_or_else(|| json!({ "refreshPath": "error" }));
                        run.record_phase(
                            "mutation.refreshWorkspace",
                            &args,
                            refresh_started.elapsed(),
                            false,
                            Some(message.clone()),
                        );
                        run.finish_error(message);
                        return Err(map_query_error(error));
                    }
                }
            }
        }

        let pre_operation_started = Instant::now();
        let mut pre_operation_accounted = Duration::ZERO;
        if action != "mutate.declare_work" && action != "mutate.checkpoint" {
            if let Some(workspace) = self.host.workspace_session_ref() {
                let flush_started = Instant::now();
                let flushed_set_count =
                    workspace.flush_observed_changes(ObservedChangeFlushTrigger::MutationBoundary);
                let flush_duration = flush_started.elapsed();
                pre_operation_accounted += flush_duration;
                run.record_phase(
                    "mutation.flushObservedChanges",
                    &json!({
                        "trigger": "mutation_boundary",
                        "flushedSetCount": flushed_set_count,
                    }),
                    flush_duration,
                    true,
                    None,
                );
            }
            let persist_started = Instant::now();
            match self
                .host
                .persist_flushed_observed_change_checkpoints_detailed(self.session.as_ref(), None)
            {
                Ok(result) => {
                    let persist_duration = persist_started.elapsed();
                    pre_operation_accounted += persist_duration;
                    run.record_phase(
                        "mutation.persistObservedChangeCheckpoints",
                        &json!({
                            "flushedSetCount": result.flushed_set_count,
                            "checkpointCount": result.event_ids.len(),
                            "changedPathCount": result.changed_path_count,
                            "entryCount": result.entry_count,
                        }),
                        persist_duration,
                        true,
                        None,
                    );
                }
                Err(error) => {
                    let mapped = map_query_error(error);
                    run.record_phase(
                        "mutation.persistObservedChangeCheckpoints",
                        &json!({}),
                        persist_started.elapsed(),
                        false,
                        Some(mapped.to_string()),
                    );
                    run.finish_error(mapped.to_string());
                    return Err(mapped);
                }
            }
        } else {
            run.record_phase(
                "mutation.flushObservedChanges",
                &json!({
                    "skipped": true,
                    "reason": "action_opted_out",
                }),
                Duration::ZERO,
                true,
                None,
            );
            run.record_phase(
                "mutation.persistObservedChangeCheckpoints",
                &json!({
                    "skipped": true,
                    "reason": "action_opted_out",
                }),
                Duration::ZERO,
                true,
                None,
            );
        }
        let pre_operation_total = pre_operation_started.elapsed();
        let pre_operation_unattributed =
            pre_operation_total.saturating_sub(pre_operation_accounted);
        if !pre_operation_unattributed.is_zero() {
            run.record_phase(
                "mutation.preOperation.unattributed",
                &json!({
                    "accountedMs": pre_operation_accounted.as_millis(),
                    "totalMs": pre_operation_total.as_millis(),
                    "action": action,
                }),
                pre_operation_unattributed,
                true,
                None,
            );
        }
        let operation_started = Instant::now();
        let (operation_result, traced_phases) =
            prism_core::mutation_trace::scope(|| operation(&run));
        for phase in traced_phases {
            run.record_phase(
                &phase.operation,
                &phase.args,
                phase.duration,
                phase.success,
                phase.error,
            );
        }
        match operation_result {
            Ok(result) => {
                run.record_phase(
                    "mutation.operation",
                    &json!({ "action": action }),
                    operation_started.elapsed(),
                    true,
                    None,
                );
                run.record_phase(
                    "mcp.executeHandler",
                    &json!({ "tool": run.tool_name(), "action": action }),
                    operation_started.elapsed(),
                    true,
                    None,
                );
                let meta = finish(&result);
                let encode_started = Instant::now();
                let result_json = match serde_json::to_value(&result) {
                    Ok(result_json) => {
                        run.record_phase(
                            "mutation.encodeResult",
                            &json!({ "action": action }),
                            encode_started.elapsed(),
                            true,
                            None,
                        );
                        run.record_phase(
                            "mcp.encodeResponse",
                            &json!({ "tool": run.tool_name(), "action": action }),
                            encode_started.elapsed(),
                            true,
                            None,
                        );
                        result_json
                    }
                    Err(error) => {
                        let mapped = map_query_error(error.into());
                        run.record_phase(
                            "mutation.encodeResult",
                            &json!({ "action": action }),
                            encode_started.elapsed(),
                            false,
                            Some(mapped.to_string()),
                        );
                        run.record_phase(
                            "mcp.encodeResponse",
                            &json!({ "tool": run.tool_name(), "action": action }),
                            encode_started.elapsed(),
                            false,
                            Some(mapped.to_string()),
                        );
                        run.finish_error(mapped.to_string());
                        return Err(mapped);
                    }
                };
                run.finish_success(
                    meta.task_id.clone(),
                    meta.result_ids,
                    meta.violation_count,
                    result_json,
                );
                Ok(result)
            }
            Err(error) => {
                let message = error.to_string();
                run.record_phase(
                    "mutation.operation",
                    &json!({ "action": action }),
                    operation_started.elapsed(),
                    false,
                    Some(message.clone()),
                );
                run.record_phase(
                    "mcp.executeHandler",
                    &json!({ "tool": run.tool_name(), "action": action }),
                    operation_started.elapsed(),
                    false,
                    Some(message.clone()),
                );
                run.finish_error(message);
                Err(map_query_error(error))
            }
        }
    }
}

#[tool_router]
impl PrismMcpServer {
    #[tool(
        description = "Locate the top 1-3 likely edit or inspection targets for an agent query using a compact result contract.",
        annotations(title = "Locate PRISM Target", read_only_hint = true),
        output_schema =
            rmcp::handler::server::tool::schema_for_output::<AgentLocateResultView>().unwrap()
    )]
    fn prism_locate(
        &self,
        Parameters(args): Parameters<PrismLocateArgs>,
    ) -> Result<CallToolResult, McpError> {
        if args.query.trim().is_empty() {
            return Err(McpError::invalid_params(
                "locate query cannot be empty",
                Some(json!({ "field": "query" })),
            ));
        }

        let result = self
            .host
            .compact_locate(Arc::clone(&self.session), args)
            .map_err(map_query_error)?;
        structured_tool_result(result)
    }

    #[tool(
        description = "Gather up to 3 bounded exact-text slices for config, schema, or script work without leaving the compact tool surface.",
        annotations(title = "Gather PRISM Slices", read_only_hint = true),
        output_schema =
            rmcp::handler::server::tool::schema_for_output::<AgentGatherResultView>().unwrap()
    )]
    fn prism_gather(
        &self,
        Parameters(args): Parameters<PrismGatherArgs>,
    ) -> Result<CallToolResult, McpError> {
        if args.query.trim().is_empty() {
            return Err(McpError::invalid_params(
                "gather query cannot be empty",
                Some(json!({ "field": "query" })),
            ));
        }

        let result = self
            .host
            .compact_gather(Arc::clone(&self.session), args)
            .map_err(map_query_error)?;
        structured_tool_result(result)
    }

    #[tool(
        description = "Open one previously located handle as a bounded focus, edit, or raw slice, or open an exact workspace path directly as a raw slice.",
        annotations(title = "Open PRISM Handle", read_only_hint = true),
        output_schema =
            rmcp::handler::server::tool::schema_for_output::<AgentOpenResultView>().unwrap()
    )]
    fn prism_open(
        &self,
        Parameters(args): Parameters<PrismOpenArgs>,
    ) -> Result<CallToolResult, McpError> {
        let has_handle = args
            .handle
            .as_ref()
            .is_some_and(|handle| !handle.trim().is_empty());
        let has_path = args
            .path
            .as_ref()
            .is_some_and(|path| !path.trim().is_empty());
        if has_handle == has_path {
            return Err(McpError::invalid_params(
                "exactly one of `handle` or `path` is required",
                Some(json!({ "fields": ["handle", "path"] })),
            ));
        }
        if args
            .handle
            .as_ref()
            .is_some_and(|handle| handle.trim().is_empty())
        {
            return Err(McpError::invalid_params(
                "handle cannot be empty",
                Some(json!({ "field": "handle" })),
            ));
        }
        if args
            .path
            .as_ref()
            .is_some_and(|path| path.trim().is_empty())
        {
            return Err(McpError::invalid_params(
                "path cannot be empty",
                Some(json!({ "field": "path" })),
            ));
        }
        if has_path && matches!(args.mode, Some(PrismOpenModeInput::Focus)) {
            return Err(McpError::invalid_params(
                "path-based prism_open currently supports raw mode, or edit mode when `line` is set",
                Some(json!({ "field": "mode" })),
            ));
        }

        let result = self
            .host
            .compact_open(Arc::clone(&self.session), args)
            .map_err(map_query_error)?;
        structured_tool_result(result)
    }

    #[tool(
        description = "Build a compact workset around one handle or query: primary target, supporting reads, likely tests, and one short why.",
        annotations(title = "PRISM Compact Workset", read_only_hint = true),
        output_schema =
            rmcp::handler::server::tool::schema_for_output::<AgentWorksetResultView>().unwrap()
    )]
    fn prism_workset(
        &self,
        Parameters(args): Parameters<PrismWorksetArgs>,
    ) -> Result<CallToolResult, McpError> {
        if args
            .handle
            .as_ref()
            .is_some_and(|handle| handle.trim().is_empty())
        {
            return Err(McpError::invalid_params(
                "handle cannot be empty",
                Some(json!({ "field": "handle" })),
            ));
        }
        if args
            .query
            .as_ref()
            .is_some_and(|query| query.trim().is_empty())
        {
            return Err(McpError::invalid_params(
                "query cannot be empty",
                Some(json!({ "field": "query" })),
            ));
        }
        if args.handle.is_none() && args.query.is_none() {
            return Err(McpError::invalid_params(
                "prism_workset requires `handle` or `query`",
                Some(json!({ "fields": ["handle", "query"] })),
            ));
        }

        let result = self
            .host
            .compact_workset(Arc::clone(&self.session), args)
            .map_err(map_query_error)?;
        structured_tool_result(result)
    }

    #[tool(
        description = "Expand one compact handle on demand with diagnostics, lineage, neighbors, diff, health, validation, impact, timeline, memory, or drift detail.",
        annotations(title = "Expand PRISM Handle", read_only_hint = true),
        output_schema =
            rmcp::handler::server::tool::schema_for_output::<AgentExpandResultView>().unwrap()
    )]
    fn prism_expand(
        &self,
        Parameters(args): Parameters<PrismExpandArgs>,
    ) -> Result<CallToolResult, McpError> {
        if args.handle.trim().is_empty() {
            return Err(McpError::invalid_params(
                "handle cannot be empty",
                Some(json!({ "field": "handle" })),
            ));
        }

        let result = self
            .host
            .compact_expand(Arc::clone(&self.session), args)
            .map_err(map_query_error)?;
        structured_tool_result(result)
    }

    #[tool(
        description = "Summarize one coordination task through the compact task lens: blockers, claim holders, recent outcomes, and suggested next reads.",
        annotations(title = "PRISM Task Brief", read_only_hint = true),
        output_schema =
            rmcp::handler::server::tool::schema_for_output::<prism_js::AgentTaskBriefResultView>().unwrap()
    )]
    fn prism_task_brief(
        &self,
        Parameters(args): Parameters<PrismTaskBriefArgs>,
    ) -> Result<CallToolResult, McpError> {
        if args.task_id.trim().is_empty() {
            return Err(McpError::invalid_params(
                "taskId cannot be empty",
                Some(json!({ "field": "taskId" })),
            ));
        }

        let result = self
            .host
            .compact_task_brief(Arc::clone(&self.session), args)
            .map_err(map_query_error)?;
        structured_tool_result(result)
    }

    #[tool(
        description = "Resolve a broad repo concept into a stable concept packet, and optionally decode it through an open, workset, validation, timeline, or memory lens.",
        annotations(title = "PRISM Concept Packet", read_only_hint = true),
        output_schema =
            rmcp::handler::server::tool::schema_for_output::<AgentConceptResultView>().unwrap()
    )]
    fn prism_concept(
        &self,
        Parameters(args): Parameters<PrismConceptArgs>,
    ) -> Result<CallToolResult, McpError> {
        if args
            .handle
            .as_ref()
            .is_some_and(|handle| handle.trim().is_empty())
        {
            return Err(McpError::invalid_params(
                "handle cannot be empty",
                Some(json!({ "field": "handle" })),
            ));
        }
        if args
            .query
            .as_ref()
            .is_some_and(|query| query.trim().is_empty())
        {
            return Err(McpError::invalid_params(
                "query cannot be empty",
                Some(json!({ "field": "query" })),
            ));
        }
        if args.handle.is_none() && args.query.is_none() {
            return Err(McpError::invalid_params(
                "prism_concept requires `handle` or `query`",
                Some(json!({ "fields": ["handle", "query"] })),
            ));
        }

        let result = self
            .host
            .compact_concept(Arc::clone(&self.session), args)
            .map_err(map_query_error)?;
        structured_tool_result(result)
    }

    #[tool(
        name = "prism_query",
        description = "Execute a read-only TypeScript query against the live PRISM runtime. Read the capabilities and schema resources for the currently available API surface.",
        annotations(title = "Programmable PRISM Query", read_only_hint = true),
        output_schema = rmcp::handler::server::tool::schema_for_output::<QueryEnvelopeSchema>()
            .unwrap()
    )]
    fn prism_query(
        &self,
        Parameters(args): Parameters<PrismQueryArgs>,
    ) -> Result<CallToolResult, McpError> {
        if args.code.trim().is_empty() {
            return Err(McpError::invalid_params(
                "query code cannot be empty",
                Some(json!({ "field": "code" })),
            ));
        }

        let language = args.language.unwrap_or(QueryLanguage::Ts);
        let envelope = self
            .host
            .execute(Arc::clone(&self.session), &args.code, language)
            .map_err(map_query_error)?;
        structured_tool_result(envelope)
    }

    #[tool(
        description = "Execute a coarse PRISM mutation. Use the tagged action union for outcomes, memory, validation feedback, inferred edges, coordination, claims, artifacts, and curator decisions.",
        annotations(
            title = "Mutate PRISM State",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        ),
        output_schema = rmcp::handler::server::tool::schema_for_output::<PrismMutationResult>()
            .unwrap()
    )]
    fn prism_mutate(
        &self,
        Parameters(args): Parameters<PrismMutationArgs>,
    ) -> Result<CallToolResult, McpError> {
        let PrismMutationArgs {
            credential,
            bridge_execution,
            mutation,
        } = args;
        let action = mutation.action_tag();
        if !self.host.features.prism_mutate_action_enabled(action) {
            return Err(McpError::invalid_params(
                "prism_mutate action is unavailable in the active runtime mode",
                Some(json!({
                    "action": action,
                    "runtimeMode": self.host.features.runtime_mode_label(),
                })),
            ));
        }
        match mutation {
            PrismMutationKindArgs::DeclareWork(args) => {
                let authenticated = self.authenticate_mutation(
                    credential.as_ref(),
                    bridge_execution.as_ref(),
                    MutationCapabilityRequirement::AnyAuthenticated,
                )?;
                let result = self.execute_logged_mutation(
                    "mutate.declare_work",
                    MutationRefreshPolicy::None,
                    || {
                        self.host.declare_work_without_refresh_authenticated(
                            self.session.as_ref(),
                            args,
                            authenticated.authenticated_principal(),
                        )
                    },
                    |result| {
                        MutationOutcomeMeta::task(
                            Some(result.work_id.clone()),
                            vec![result.work_id.clone()],
                            0,
                        )
                    },
                )?;
                structured_tool_result(PrismMutationResult {
                    action: PrismMutationActionSchema::DeclareWork,
                    result: serde_json::to_value(result)
                        .map_err(|err| map_query_error(err.into()))?,
                })
            }
            PrismMutationKindArgs::Checkpoint(args) => {
                let authenticated = self.authenticate_mutation(
                    credential.as_ref(),
                    bridge_execution.as_ref(),
                    MutationCapabilityRequirement::MutateRepoMemory,
                )?;
                self.require_declared_work_context("checkpoint", args.task_id.is_some())?;
                let result = self.execute_logged_mutation(
                    "mutate.checkpoint",
                    MutationRefreshPolicy::None,
                    || {
                        self.host.store_checkpoint_authenticated(
                            self.session.as_ref(),
                            args,
                            authenticated.authenticated_principal(),
                        )
                    },
                    |result| {
                        let mut result_ids = result.event_ids.clone();
                        result_ids.push(result.task_id.clone());
                        MutationOutcomeMeta::task(Some(result.task_id.clone()), result_ids, 0)
                    },
                )?;
                structured_tool_result(PrismMutationResult {
                    action: PrismMutationActionSchema::Checkpoint,
                    result: serde_json::to_value(result)
                        .map_err(|err| map_query_error(err.into()))?,
                })
            }
            PrismMutationKindArgs::Outcome(args) => {
                let authenticated = self.authenticate_mutation(
                    credential.as_ref(),
                    bridge_execution.as_ref(),
                    MutationCapabilityRequirement::MutateRepoMemory,
                )?;
                self.require_declared_work_context("outcome", args.task_id.is_some())?;
                let result = self.execute_logged_mutation(
                    "mutate.outcome",
                    MutationRefreshPolicy::None,
                    || {
                        self.host.store_outcome_without_refresh_authenticated(
                            self.session.as_ref(),
                            args,
                            authenticated.authenticated_principal(),
                        )
                    },
                    |result| {
                        MutationOutcomeMeta::task(
                            Some(result.task_id.clone()),
                            vec![result.task_id.clone(), result.event_id.clone()],
                            0,
                        )
                    },
                )?;
                structured_tool_result_with_links(
                    PrismMutationResult {
                        action: PrismMutationActionSchema::Outcome,
                        result: serde_json::to_value(result.clone())
                            .map_err(|err| map_query_error(err.into()))?,
                    },
                    vec![
                        event_resource_link(&result.event_id),
                        task_resource_link(&result.task_id),
                    ],
                )
            }
            PrismMutationKindArgs::Memory(args) => {
                let authenticated = self.authenticate_mutation(
                    credential.as_ref(),
                    bridge_execution.as_ref(),
                    MutationCapabilityRequirement::MutateRepoMemory,
                )?;
                self.require_declared_work_context("memory", args.task_id.is_some())?;
                let result = self.execute_logged_mutation(
                    "mutate.memory",
                    MutationRefreshPolicy::None,
                    || {
                        self.host.store_memory_without_refresh_authenticated(
                            self.session.as_ref(),
                            args,
                            authenticated.authenticated_principal(),
                        )
                    },
                    |result| {
                        MutationOutcomeMeta::task(
                            Some(result.task_id.clone()),
                            vec![result.task_id.clone(), result.memory_id.clone()],
                            0,
                        )
                    },
                )?;
                structured_tool_result_with_links(
                    PrismMutationResult {
                        action: PrismMutationActionSchema::Memory,
                        result: serde_json::to_value(result.clone())
                            .map_err(|err| map_query_error(err.into()))?,
                    },
                    vec![
                        memory_resource_link(&result.memory_id),
                        task_resource_link(&result.task_id),
                    ],
                )
            }
            PrismMutationKindArgs::Concept(args) => {
                let authenticated = self.authenticate_mutation(
                    credential.as_ref(),
                    bridge_execution.as_ref(),
                    MutationCapabilityRequirement::MutateRepoMemory,
                )?;
                self.require_declared_work_context("concept", args.task_id.is_some())?;
                let result = self.execute_logged_mutation(
                    "mutate.concept",
                    MutationRefreshPolicy::None,
                    || {
                        self.host.store_concept_without_refresh_authenticated(
                            self.session.as_ref(),
                            args,
                            authenticated.authenticated_principal(),
                        )
                    },
                    |result| {
                        MutationOutcomeMeta::task(
                            Some(result.task_id.clone()),
                            vec![
                                result.task_id.clone(),
                                result.event_id.clone(),
                                result.concept_handle.clone(),
                            ],
                            0,
                        )
                    },
                )?;
                structured_tool_result_with_links(
                    PrismMutationResult {
                        action: PrismMutationActionSchema::Concept,
                        result: serde_json::to_value(result.clone())
                            .map_err(|err| map_query_error(err.into()))?,
                    },
                    vec![task_resource_link(&result.task_id)],
                )
            }
            PrismMutationKindArgs::Contract(args) => {
                let authenticated = self.authenticate_mutation(
                    credential.as_ref(),
                    bridge_execution.as_ref(),
                    MutationCapabilityRequirement::MutateRepoMemory,
                )?;
                self.require_declared_work_context("contract", args.task_id.is_some())?;
                let result = self.execute_logged_mutation(
                    "mutate.contract",
                    MutationRefreshPolicy::None,
                    || {
                        self.host.store_contract_without_refresh_authenticated(
                            self.session.as_ref(),
                            args,
                            authenticated.authenticated_principal(),
                        )
                    },
                    |result| {
                        MutationOutcomeMeta::task(
                            Some(result.task_id.clone()),
                            vec![
                                result.task_id.clone(),
                                result.event_id.clone(),
                                result.contract_handle.clone(),
                            ],
                            0,
                        )
                    },
                )?;
                structured_tool_result_with_links(
                    PrismMutationResult {
                        action: PrismMutationActionSchema::Contract,
                        result: serde_json::to_value(result.clone())
                            .map_err(|err| map_query_error(err.into()))?,
                    },
                    vec![task_resource_link(&result.task_id)],
                )
            }
            PrismMutationKindArgs::ConceptRelation(args) => {
                let authenticated = self.authenticate_mutation(
                    credential.as_ref(),
                    bridge_execution.as_ref(),
                    MutationCapabilityRequirement::MutateRepoMemory,
                )?;
                self.require_declared_work_context("concept_relation", args.task_id.is_some())?;
                let result = self.execute_logged_mutation(
                    "mutate.concept_relation",
                    MutationRefreshPolicy::None,
                    || {
                        self.host.store_concept_relation_authenticated(
                            self.session.as_ref(),
                            args,
                            authenticated.authenticated_principal(),
                        )
                    },
                    |result| {
                        MutationOutcomeMeta::task(
                            Some(result.task_id.clone()),
                            vec![
                                result.task_id.clone(),
                                result.event_id.clone(),
                                result.relation.related_handle.clone(),
                            ],
                            0,
                        )
                    },
                )?;
                structured_tool_result_with_links(
                    PrismMutationResult {
                        action: PrismMutationActionSchema::ConceptRelation,
                        result: serde_json::to_value(result.clone())
                            .map_err(|err| map_query_error(err.into()))?,
                    },
                    vec![task_resource_link(&result.task_id)],
                )
            }
            PrismMutationKindArgs::ValidationFeedback(args) => {
                let authenticated = self.authenticate_mutation(
                    credential.as_ref(),
                    bridge_execution.as_ref(),
                    MutationCapabilityRequirement::MutateRepoMemory,
                )?;
                self.require_declared_work_context("validation_feedback", args.task_id.is_some())?;
                let result = self.execute_logged_mutation(
                    "mutate.validation_feedback",
                    MutationRefreshPolicy::None,
                    || {
                        self.host
                            .store_validation_feedback_without_refresh_authenticated(
                                self.session.as_ref(),
                                args,
                                authenticated.authenticated_principal(),
                            )
                    },
                    |result| {
                        MutationOutcomeMeta::task(
                            Some(result.task_id.clone()),
                            vec![result.task_id.clone(), result.entry_id.clone()],
                            0,
                        )
                    },
                )?;
                structured_tool_result_with_links(
                    PrismMutationResult {
                        action: PrismMutationActionSchema::ValidationFeedback,
                        result: serde_json::to_value(result.clone())
                            .map_err(|err| map_query_error(err.into()))?,
                    },
                    vec![task_resource_link(&result.task_id)],
                )
            }
            PrismMutationKindArgs::SessionRepair(args) => {
                self.authenticate_mutation(
                    credential.as_ref(),
                    bridge_execution.as_ref(),
                    MutationCapabilityRequirement::AnyAuthenticated,
                )?;
                let result = self.execute_logged_mutation(
                    "mutate.session_repair",
                    MutationRefreshPolicy::None,
                    || {
                        self.host
                            .repair_session_without_refresh(self.session.as_ref(), args)
                    },
                    |result| {
                        MutationOutcomeMeta::task(
                            result.cleared_task_id.clone(),
                            result.cleared_task_id.clone().into_iter().collect(),
                            0,
                        )
                    },
                )?;
                let mut links = Vec::new();
                if let Some(task_id) = &result.cleared_task_id {
                    links.push(task_resource_link(task_id));
                }
                structured_tool_result_with_links(
                    PrismMutationResult {
                        action: PrismMutationActionSchema::SessionRepair,
                        result: serde_json::to_value(result.clone())
                            .map_err(|err| map_query_error(err.into()))?,
                    },
                    links,
                )
            }
            PrismMutationKindArgs::InferEdge(args) => {
                self.authenticate_mutation(
                    credential.as_ref(),
                    bridge_execution.as_ref(),
                    MutationCapabilityRequirement::MutateRepoMemory,
                )?;
                self.require_declared_work_context("infer_edge", args.task_id.is_some())?;
                let result = self.execute_logged_mutation(
                    "mutate.infer_edge",
                    MutationRefreshPolicy::None,
                    || self.host.store_inferred_edge(self.session.as_ref(), args),
                    |result| {
                        MutationOutcomeMeta::task(
                            Some(result.task_id.clone()),
                            vec![result.task_id.clone(), result.edge_id.clone()],
                            0,
                        )
                    },
                )?;
                structured_tool_result_with_links(
                    PrismMutationResult {
                        action: PrismMutationActionSchema::InferEdge,
                        result: serde_json::to_value(result.clone())
                            .map_err(|err| map_query_error(err.into()))?,
                    },
                    vec![
                        edge_resource_link(&result.edge_id),
                        task_resource_link(&result.task_id),
                    ],
                )
            }
            PrismMutationKindArgs::HeartbeatLease(args) => {
                self.host
                    .ensure_tool_enabled("prism_coordination", "coordination lease heartbeats")
                    .map_err(map_query_error)?;
                let authenticated = self.authenticate_mutation(
                    credential.as_ref(),
                    bridge_execution.as_ref(),
                    MutationCapabilityRequirement::MutateCoordination,
                )?;
                self.require_declared_work_context(
                    "heartbeat_lease",
                    args.task_id.is_some() || args.claim_id.is_some(),
                )?;
                let result = self.execute_logged_mutation(
                    "mutate.heartbeat_lease",
                    MutationRefreshPolicy::None,
                    || {
                        self.host.store_heartbeat_lease_authenticated(
                            self.session.as_ref(),
                            args,
                            authenticated.authenticated_principal(),
                        )
                    },
                    |result| {
                        let mut result_ids = result.event_ids.clone();
                        if let Some(task_id) = &result.task_id {
                            result_ids.push(task_id.clone());
                        }
                        if let Some(claim_id) = &result.claim_id {
                            result_ids.push(claim_id.clone());
                        }
                        MutationOutcomeMeta::coordination(result_ids, result.violations.len())
                    },
                )?;
                structured_tool_result(PrismMutationResult {
                    action: PrismMutationActionSchema::HeartbeatLease,
                    result: serde_json::to_value(result)
                        .map_err(|err| map_query_error(err.into()))?,
                })
            }
            PrismMutationKindArgs::Coordination(args) => {
                self.host
                    .ensure_tool_enabled("prism_coordination", "coordination workflow mutations")
                    .map_err(map_query_error)?;
                let run = self
                    .host
                    .begin_mutation_run(self.session.as_ref(), "mutate.coordination");
                let authenticated = match self.authenticate_mutation_with_run(
                    &run,
                    credential.as_ref(),
                    bridge_execution.as_ref(),
                    MutationCapabilityRequirement::MutateCoordination,
                ) {
                    Ok(authenticated) => authenticated,
                    Err(error) => {
                        run.finish_error(error.to_string());
                        return Err(error);
                    }
                };
                if let Err(error) = self.require_declared_work_context_with_run(
                    &run,
                    "coordination",
                    Self::coordination_has_explicit_work_subject(&args),
                ) {
                    run.finish_error(error.to_string());
                    return Err(error);
                }
                let result = self.execute_logged_mutation_with_existing_run(
                    run,
                    "mutate.coordination",
                    MutationRefreshPolicy::None,
                    |run| {
                        self.host.store_coordination_traced_authenticated(
                            self.session.as_ref(),
                            args,
                            run,
                            authenticated.authenticated_principal(),
                        )
                    },
                    |result| {
                        MutationOutcomeMeta::coordination(
                            result.event_ids.clone(),
                            result.violations.len(),
                        )
                    },
                )?;
                structured_tool_result(PrismMutationResult {
                    action: PrismMutationActionSchema::Coordination,
                    result: serde_json::to_value(result)
                        .map_err(|err| map_query_error(err.into()))?,
                })
            }
            PrismMutationKindArgs::Claim(args) => {
                self.host
                    .ensure_tool_enabled("prism_claim", "coordination claim mutations")
                    .map_err(map_query_error)?;
                let authenticated = self.authenticate_mutation(
                    credential.as_ref(),
                    bridge_execution.as_ref(),
                    MutationCapabilityRequirement::MutateCoordination,
                )?;
                self.require_declared_work_context("claim", args.task_id.is_some())?;
                let result = self.execute_logged_mutation(
                    "mutate.claim",
                    MutationRefreshPolicy::None,
                    || {
                        self.host.store_claim_authenticated(
                            self.session.as_ref(),
                            args,
                            authenticated.authenticated_principal(),
                        )
                    },
                    |result| {
                        let mut result_ids = result.event_ids.clone();
                        if let Some(claim_id) = &result.claim_id {
                            result_ids.push(claim_id.clone());
                        }
                        MutationOutcomeMeta::coordination(result_ids, result.violations.len())
                    },
                )?;
                structured_tool_result(PrismMutationResult {
                    action: PrismMutationActionSchema::Claim,
                    result: serde_json::to_value(result)
                        .map_err(|err| map_query_error(err.into()))?,
                })
            }
            PrismMutationKindArgs::Artifact(args) => {
                self.host
                    .ensure_tool_enabled("prism_artifact", "coordination artifact mutations")
                    .map_err(map_query_error)?;
                let authenticated = self.authenticate_mutation(
                    credential.as_ref(),
                    bridge_execution.as_ref(),
                    MutationCapabilityRequirement::MutateCoordination,
                )?;
                self.require_declared_work_context("artifact", args.task_id.is_some())?;
                let result = self.execute_logged_mutation(
                    "mutate.artifact",
                    MutationRefreshPolicy::None,
                    || {
                        self.host.store_artifact_authenticated(
                            self.session.as_ref(),
                            args,
                            authenticated.authenticated_principal(),
                        )
                    },
                    |result| {
                        let mut result_ids = result.event_ids.clone();
                        if let Some(artifact_id) = &result.artifact_id {
                            result_ids.push(artifact_id.clone());
                        }
                        if let Some(review_id) = &result.review_id {
                            result_ids.push(review_id.clone());
                        }
                        MutationOutcomeMeta::coordination(result_ids, result.violations.len())
                    },
                )?;
                structured_tool_result(PrismMutationResult {
                    action: PrismMutationActionSchema::Artifact,
                    result: serde_json::to_value(result)
                        .map_err(|err| map_query_error(err.into()))?,
                })
            }
            PrismMutationKindArgs::TestRan(args) => {
                let authenticated = self.authenticate_mutation(
                    credential.as_ref(),
                    bridge_execution.as_ref(),
                    MutationCapabilityRequirement::MutateRepoMemory,
                )?;
                self.require_declared_work_context("test_ran", args.task_id.is_some())?;
                let summary = format!(
                    "test `{}` {}",
                    args.test,
                    if args.passed { "passed" } else { "failed" }
                );
                let mut evidence = vec![OutcomeEvidenceInput::Test {
                    name: args.test.clone(),
                    passed: args.passed,
                }];
                if let Some(command) = args.command.clone() {
                    evidence.push(OutcomeEvidenceInput::Command {
                        argv: command,
                        passed: args.passed,
                    });
                }
                let result = self.execute_logged_mutation(
                    "mutate.test_ran",
                    MutationRefreshPolicy::None,
                    || {
                        self.host.store_outcome_without_refresh_authenticated(
                            self.session.as_ref(),
                            PrismOutcomeArgs {
                                kind: OutcomeKindInput::TestRan,
                                anchors: args.anchors,
                                summary,
                                result: Some(if args.passed {
                                    OutcomeResultInput::Success
                                } else {
                                    OutcomeResultInput::Failure
                                }),
                                evidence: Some(evidence),
                                task_id: args.task_id,
                            },
                            authenticated.authenticated_principal(),
                        )
                    },
                    |result| {
                        MutationOutcomeMeta::task(
                            Some(result.task_id.clone()),
                            vec![result.task_id.clone(), result.event_id.clone()],
                            0,
                        )
                    },
                )?;
                structured_tool_result_with_links(
                    PrismMutationResult {
                        action: PrismMutationActionSchema::TestRan,
                        result: serde_json::to_value(result.clone())
                            .map_err(|err| map_query_error(err.into()))?,
                    },
                    vec![
                        event_resource_link(&result.event_id),
                        task_resource_link(&result.task_id),
                    ],
                )
            }
            PrismMutationKindArgs::FailureObserved(args) => {
                let authenticated = self.authenticate_mutation(
                    credential.as_ref(),
                    bridge_execution.as_ref(),
                    MutationCapabilityRequirement::MutateRepoMemory,
                )?;
                self.require_declared_work_context("failure_observed", args.task_id.is_some())?;
                let evidence = args
                    .trace
                    .map(|trace| vec![OutcomeEvidenceInput::StackTrace { hash: trace }]);
                let result = self.execute_logged_mutation(
                    "mutate.failure_observed",
                    MutationRefreshPolicy::None,
                    || {
                        self.host.store_outcome_without_refresh_authenticated(
                            self.session.as_ref(),
                            PrismOutcomeArgs {
                                kind: OutcomeKindInput::FailureObserved,
                                anchors: args.anchors,
                                summary: args.summary,
                                result: Some(OutcomeResultInput::Failure),
                                evidence,
                                task_id: args.task_id,
                            },
                            authenticated.authenticated_principal(),
                        )
                    },
                    |result| {
                        MutationOutcomeMeta::task(
                            Some(result.task_id.clone()),
                            vec![result.task_id.clone(), result.event_id.clone()],
                            0,
                        )
                    },
                )?;
                structured_tool_result_with_links(
                    PrismMutationResult {
                        action: PrismMutationActionSchema::FailureObserved,
                        result: serde_json::to_value(result.clone())
                            .map_err(|err| map_query_error(err.into()))?,
                    },
                    vec![
                        event_resource_link(&result.event_id),
                        task_resource_link(&result.task_id),
                    ],
                )
            }
            PrismMutationKindArgs::FixValidated(args) => {
                let authenticated = self.authenticate_mutation(
                    credential.as_ref(),
                    bridge_execution.as_ref(),
                    MutationCapabilityRequirement::MutateRepoMemory,
                )?;
                self.require_declared_work_context("fix_validated", args.task_id.is_some())?;
                let evidence = args.command.clone().map(|command| {
                    vec![OutcomeEvidenceInput::Command {
                        argv: command,
                        passed: true,
                    }]
                });
                let result = self.execute_logged_mutation(
                    "mutate.fix_validated",
                    MutationRefreshPolicy::None,
                    || {
                        self.host.store_outcome_without_refresh_authenticated(
                            self.session.as_ref(),
                            PrismOutcomeArgs {
                                kind: OutcomeKindInput::FixValidated,
                                anchors: args.anchors,
                                summary: args.summary,
                                result: Some(OutcomeResultInput::Success),
                                evidence,
                                task_id: args.task_id,
                            },
                            authenticated.authenticated_principal(),
                        )
                    },
                    |result| {
                        MutationOutcomeMeta::task(
                            Some(result.task_id.clone()),
                            vec![result.task_id.clone(), result.event_id.clone()],
                            0,
                        )
                    },
                )?;
                structured_tool_result_with_links(
                    PrismMutationResult {
                        action: PrismMutationActionSchema::FixValidated,
                        result: serde_json::to_value(result.clone())
                            .map_err(|err| map_query_error(err.into()))?,
                    },
                    vec![
                        event_resource_link(&result.event_id),
                        task_resource_link(&result.task_id),
                    ],
                )
            }
            PrismMutationKindArgs::CuratorPromoteEdge(args) => {
                self.authenticate_mutation(
                    credential.as_ref(),
                    bridge_execution.as_ref(),
                    MutationCapabilityRequirement::MutateRepoMemory,
                )?;
                self.require_declared_work_context("curator_promote_edge", args.task_id.is_some())?;
                let result = self.execute_logged_mutation(
                    "mutate.curator_promote_edge",
                    MutationRefreshPolicy::None,
                    || self.host.promote_curator_edge(self.session.as_ref(), args),
                    |result| {
                        let mut result_ids = vec![result.job_id.clone()];
                        if let Some(memory_id) = &result.memory_id {
                            result_ids.push(memory_id.clone());
                        }
                        if let Some(edge_id) = &result.edge_id {
                            result_ids.push(edge_id.clone());
                        }
                        MutationOutcomeMeta::coordination(result_ids, 0)
                    },
                )?;
                let mut links = vec![session_resource_link()];
                if let Some(memory_id) = &result.memory_id {
                    links.push(memory_resource_link(memory_id));
                }
                if let Some(edge_id) = &result.edge_id {
                    links.push(edge_resource_link(edge_id));
                }
                structured_tool_result_with_links(
                    PrismMutationResult {
                        action: PrismMutationActionSchema::CuratorPromoteEdge,
                        result: serde_json::to_value(result)
                            .map_err(|err| map_query_error(err.into()))?,
                    },
                    links,
                )
            }
            PrismMutationKindArgs::CuratorApplyProposal(args) => {
                let authenticated = self.authenticate_mutation(
                    credential.as_ref(),
                    bridge_execution.as_ref(),
                    MutationCapabilityRequirement::MutateRepoMemory,
                )?;
                self.require_declared_work_context(
                    "curator_apply_proposal",
                    args.task_id.is_some(),
                )?;
                let result = self.execute_logged_mutation(
                    "mutate.curator_apply_proposal",
                    MutationRefreshPolicy::None,
                    || {
                        self.host.apply_curator_proposal_authenticated(
                            self.session.as_ref(),
                            args,
                            authenticated.authenticated_principal(),
                        )
                    },
                    |result| {
                        let mut result_ids = vec![result.job_id.clone()];
                        if let Some(memory_id) = &result.memory_id {
                            result_ids.push(memory_id.clone());
                        }
                        if let Some(edge_id) = &result.edge_id {
                            result_ids.push(edge_id.clone());
                        }
                        if let Some(concept_handle) = &result.concept_handle {
                            result_ids.push(concept_handle.clone());
                        }
                        MutationOutcomeMeta::coordination(result_ids, 0)
                    },
                )?;
                let mut links = vec![session_resource_link()];
                if let Some(memory_id) = &result.memory_id {
                    links.push(memory_resource_link(memory_id));
                }
                if let Some(edge_id) = &result.edge_id {
                    links.push(edge_resource_link(edge_id));
                }
                structured_tool_result_with_links(
                    PrismMutationResult {
                        action: PrismMutationActionSchema::CuratorApplyProposal,
                        result: serde_json::to_value(result)
                            .map_err(|err| map_query_error(err.into()))?,
                    },
                    links,
                )
            }
            PrismMutationKindArgs::CuratorPromoteConcept(args) => {
                let authenticated = self.authenticate_mutation(
                    credential.as_ref(),
                    bridge_execution.as_ref(),
                    MutationCapabilityRequirement::MutateRepoMemory,
                )?;
                self.require_declared_work_context(
                    "curator_promote_concept",
                    args.task_id.is_some(),
                )?;
                let result = self.execute_logged_mutation(
                    "mutate.curator_promote_concept",
                    MutationRefreshPolicy::None,
                    || {
                        self.host.promote_curator_concept_authenticated(
                            self.session.as_ref(),
                            args,
                            authenticated.authenticated_principal(),
                        )
                    },
                    |result| {
                        let mut result_ids = vec![result.job_id.clone()];
                        if let Some(concept_handle) = &result.concept_handle {
                            result_ids.push(concept_handle.clone());
                        }
                        MutationOutcomeMeta::coordination(result_ids, 0)
                    },
                )?;
                structured_tool_result_with_links(
                    PrismMutationResult {
                        action: PrismMutationActionSchema::CuratorPromoteConcept,
                        result: serde_json::to_value(result)
                            .map_err(|err| map_query_error(err.into()))?,
                    },
                    vec![session_resource_link()],
                )
            }
            PrismMutationKindArgs::CuratorPromoteMemory(args) => {
                let authenticated = self.authenticate_mutation(
                    credential.as_ref(),
                    bridge_execution.as_ref(),
                    MutationCapabilityRequirement::MutateRepoMemory,
                )?;
                self.require_declared_work_context(
                    "curator_promote_memory",
                    args.task_id.is_some(),
                )?;
                let result = self.execute_logged_mutation(
                    "mutate.curator_promote_memory",
                    MutationRefreshPolicy::None,
                    || {
                        self.host.promote_curator_memory_authenticated(
                            self.session.as_ref(),
                            args,
                            authenticated.authenticated_principal(),
                        )
                    },
                    |result| {
                        let mut result_ids = vec![result.job_id.clone()];
                        if let Some(memory_id) = &result.memory_id {
                            result_ids.push(memory_id.clone());
                        }
                        if let Some(edge_id) = &result.edge_id {
                            result_ids.push(edge_id.clone());
                        }
                        MutationOutcomeMeta::coordination(result_ids, 0)
                    },
                )?;
                let mut links = vec![session_resource_link()];
                if let Some(memory_id) = &result.memory_id {
                    links.push(memory_resource_link(memory_id));
                }
                if let Some(edge_id) = &result.edge_id {
                    links.push(edge_resource_link(edge_id));
                }
                structured_tool_result_with_links(
                    PrismMutationResult {
                        action: PrismMutationActionSchema::CuratorPromoteMemory,
                        result: serde_json::to_value(result)
                            .map_err(|err| map_query_error(err.into()))?,
                    },
                    links,
                )
            }
            PrismMutationKindArgs::CuratorRejectProposal(args) => {
                let authenticated = self.authenticate_mutation(
                    credential.as_ref(),
                    bridge_execution.as_ref(),
                    MutationCapabilityRequirement::MutateRepoMemory,
                )?;
                self.require_declared_work_context(
                    "curator_reject_proposal",
                    args.task_id.is_some(),
                )?;
                let result = self.execute_logged_mutation(
                    "mutate.curator_reject_proposal",
                    MutationRefreshPolicy::None,
                    || {
                        self.host.reject_curator_proposal_authenticated(
                            self.session.as_ref(),
                            args,
                            authenticated.authenticated_principal(),
                        )
                    },
                    |result| MutationOutcomeMeta::coordination(vec![result.job_id.clone()], 0),
                )?;
                structured_tool_result_with_links(
                    PrismMutationResult {
                        action: PrismMutationActionSchema::CuratorRejectProposal,
                        result: serde_json::to_value(result)
                            .map_err(|err| map_query_error(err.into()))?,
                    },
                    vec![session_resource_link()],
                )
            }
        }
    }
}

impl ServerHandler for PrismMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_resources()
                .enable_tools()
                .build(),
        )
        .with_server_info(Implementation::from_build_env())
        .with_instructions(self.server_instructions())
        .with_protocol_version(ProtocolVersion::LATEST)
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let started_at = current_timestamp();
        let started = Instant::now();
        let tool_name = request.name.to_string();
        let request_preview = Some(json!({
            "name": tool_name,
            "arguments": request.arguments.clone(),
        }));
        if !self.host.features.is_tool_enabled(request.name.as_ref()) {
            let error =
                McpError::invalid_params("tool not found", Some(json!({ "name": request.name })));
            self.record_surface_call(
                "tool",
                &tool_name,
                format!("call {tool_name}"),
                started_at,
                started,
                false,
                Some(error.to_string()),
                request_preview,
                None,
                json!({ "name": tool_name }),
                Vec::new(),
            );
            return Err(error);
        }
        let context = rmcp::handler::server::tool::ToolCallContext::new(self, request, context);
        self.tool_router.call(context).await
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let started_at = current_timestamp();
        let started = Instant::now();
        let result = ListToolsResult {
            tools: self
                .tool_router
                .list_all()
                .into_iter()
                .filter(|tool| self.host.features.is_tool_enabled(&tool.name))
                .map(|tool| Self::transport_bind_tool_schema(tool, &self.host.features))
                .collect(),
            next_cursor: None,
            meta: None,
        };
        self.record_surface_call(
            "tool_list",
            "list_tools",
            "list tools".to_string(),
            started_at,
            started,
            true,
            None,
            Some(json!({ "method": "list_tools" })),
            Some(json!({
                "count": result.tools.len(),
                "names": result
                    .tools
                    .iter()
                    .take(8)
                    .map(|tool| tool.name.clone())
                    .collect::<Vec<_>>(),
            })),
            json!({ "method": "list_tools" }),
            Vec::new(),
        );
        Ok(result)
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.host
            .features
            .is_tool_enabled(name)
            .then(|| {
                self.tool_router
                    .get(name)
                    .cloned()
                    .map(|tool| Self::transport_bind_tool_schema(tool, &self.host.features))
            })
            .flatten()
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        let started_at = current_timestamp();
        let started = Instant::now();
        let mut resources = vec![instructions_resource_link()
            .with_title("PRISM Instruction Sets")
            .no_annotation()];
        resources.extend(
            instruction_set_resource_links(self.host.features.runtime_mode())
                .into_iter()
                .map(|resource| resource.no_annotation()),
        );
        if self.host.features.cognition_layer_enabled() {
            resources.push(
                RawResource::new(API_REFERENCE_URI, "PRISM API Reference")
                    .with_description(
                        "TypeScript query surface, d.ts-style contract, and usage recipes",
                    )
                    .with_mime_type("text/markdown")
                    .with_title("PRISM API Reference")
                    .no_annotation(),
            );
        }
        resources.extend([
            capabilities_resource_link()
                .with_title("PRISM Capabilities")
                .no_annotation(),
            RawResource::new(SESSION_URI, "PRISM Session")
                .with_description(
                    "Active workspace root, current task context, and runtime query limits",
                )
                .with_mime_type("application/json")
                .with_title("PRISM Session")
                .with_meta(resource_meta(
                    "session",
                    Some(schema_resource_uri("session")),
                    None,
                ))
                .no_annotation(),
            plans_resource_link()
                .with_title("PRISM Plans")
                .no_annotation(),
        ]);
        resources.extend([
            protected_state_resource_link()
                .with_title("PRISM Protected State")
                .no_annotation(),
            RawResource::new(VOCAB_URI, "PRISM Vocabulary")
                .with_description(
                    "Canonical enum and action vocabularies for PRISM MCP resources, query args, and mutation payloads",
                )
                .with_mime_type("application/json")
                .with_title("PRISM Vocabulary")
                .with_meta(resource_meta(
                    "vocab",
                    Some(schema_resource_uri("vocab")),
                    None,
                ))
                .no_annotation(),
            RawResource::new(TOOL_SCHEMAS_URI, "PRISM Tool Schemas")
                .with_description("Catalog of JSON Schemas for PRISM MCP tool input payloads")
                .with_mime_type("application/json")
                .with_title("PRISM Tool Schemas")
                .with_meta(resource_meta(
                    "tool-schemas",
                    Some(schema_resource_uri("tool-schemas")),
                    None,
                ))
                .no_annotation(),
        ]);
        if self.host.features.cognition_layer_enabled() {
            resources.extend([
                contracts_resource_link()
                    .with_title("PRISM Contracts")
                    .no_annotation(),
                RawResource::new(ENTRYPOINTS_URI, "PRISM Entrypoints")
                    .with_description(
                        "Workspace entrypoints and top-level starting symbols in structured JSON, with optional cursor-based pagination",
                    )
                    .with_mime_type("application/json")
                    .with_title("PRISM Entrypoints")
                    .with_meta(resource_meta(
                        "entrypoints",
                        Some(schema_resource_uri("entrypoints")),
                        None,
                    ))
                    .no_annotation(),
                RawResource::new(SCHEMAS_URI, "PRISM Resource Schemas")
                    .with_description(
                        "Catalog of JSON Schemas for all structured PRISM resource payloads",
                    )
                    .with_mime_type("application/json")
                    .with_title("PRISM Resource Schemas")
                    .with_meta(resource_meta(
                        "schemas",
                        Some(schema_resource_uri("schemas")),
                        None,
                    ))
                    .no_annotation(),
                self_description_audit_resource_link()
                    .with_title("PRISM Self-Description Audit")
                    .no_annotation(),
            ]);
        }
        let result = ListResourcesResult {
            resources,
            next_cursor: None,
            meta: None,
        };
        self.record_surface_call(
            "resource_list",
            "list_resources",
            "list resources".to_string(),
            started_at,
            started,
            true,
            None,
            Some(json!({ "method": "list_resources" })),
            Some(json!({
                "count": result.resources.len(),
                "uris": result
                    .resources
                    .iter()
                    .take(8)
                    .map(|resource| resource.uri.clone())
                    .collect::<Vec<_>>(),
            })),
            json!({ "method": "list_resources" }),
            Vec::new(),
        );
        Ok(result)
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, McpError> {
        let started_at = current_timestamp();
        let started = Instant::now();
        let uri = request.uri.as_str();
        let (base_uri, _) = split_resource_uri(uri);
        let resource_not_found =
            || McpError::resource_not_found("resource_not_found", Some(json!({ "uri": uri })));
        let require_resource_kind = |resource_kind: &str| {
            if self.host.features.resource_kind_visible(resource_kind) {
                Ok(())
            } else {
                Err(resource_not_found())
            }
        };
        let require_tool_name = |tool_name: &str| {
            if self.host.features.is_tool_enabled(tool_name) {
                Ok(())
            } else {
                Err(resource_not_found())
            }
        };
        let require_tool_examples = || {
            if self.host.features.tool_example_resources_visible() {
                Ok(())
            } else {
                Err(resource_not_found())
            }
        };
        let require_resource_examples = || {
            if self.host.features.resource_example_resources_visible() {
                Ok(())
            } else {
                Err(resource_not_found())
            }
        };
        let require_tool_action_name = |tool_name: &str, action: &str| {
            require_tool_name(tool_name)?;
            if tool_name == "prism_mutate"
                && !self.host.features.prism_mutate_action_enabled(action)
            {
                return Err(resource_not_found());
            }
            Ok(())
        };
        let (contents_result, resource_trace) =
            crate::resource_trace::ResourceTraceState::scope(async {
                (|| -> Result<ResourceContents, McpError> {
                    Ok(if let Some(instruction_set_id) =
                        crate::instructions::parse_instruction_resource_uri(uri)
                    {
                        let markdown = match instruction_set_id {
                            None => self.server_instructions(),
                            Some(id) => crate::instructions::render_instruction_set_with_features(
                                &id,
                                &self.host.features,
                            )
                            .ok_or_else(|| {
                                McpError::resource_not_found(
                                    "resource_not_found",
                                    Some(json!({ "uri": request.uri })),
                                )
                            })?,
                        };
                        ResourceContents::text(markdown, request.uri.clone())
                            .with_mime_type("text/markdown")
                    } else if base_uri == API_REFERENCE_URI {
                        if !self.host.features.cognition_layer_enabled() {
                            return Err(McpError::resource_not_found(
                                "resource_not_found",
                                Some(json!({ "uri": request.uri })),
                            ));
                        }
                        ResourceContents::text(
                            self.host.api_reference_markdown(),
                            request.uri.clone(),
                        )
                        .with_mime_type("text/markdown")
                    } else if base_uri == CAPABILITIES_URI {
                        json_resource_contents_with_meta(
                            self.host
                                .capabilities_resource_value()
                                .map_err(map_query_error)?,
                            request.uri.clone(),
                            Some(resource_meta(
                                "capabilities",
                                Some(schema_resource_uri("capabilities")),
                                None,
                            )),
                        )?
                    } else if let Some(section) = parse_capabilities_section_resource_uri(uri) {
                        capabilities_section_resource_contents(
                            self.host
                                .capabilities_resource_value()
                                .map_err(map_query_error)?,
                            &section,
                            request.uri.as_str(),
                        )?
                    } else if base_uri == PROTECTED_STATE_URI {
                        require_resource_kind("protected-state")?;
                        json_resource_contents_with_meta(
                            self.host
                                .protected_state_resource_value(uri)
                                .map_err(map_query_error)?,
                            request.uri.clone(),
                            Some(resource_meta(
                                "protected-state",
                                Some(schema_resource_uri("protected-state")),
                                None,
                            )),
                        )?
                    } else if base_uri == VOCAB_URI {
                        require_resource_kind("vocab")?;
                        json_resource_contents_with_meta(
                            self.host.vocab_resource_value(),
                            request.uri.clone(),
                            Some(resource_meta(
                                "vocab",
                                Some(schema_resource_uri("vocab")),
                                None,
                            )),
                        )?
                    } else if let Some(key) = parse_vocab_entry_resource_uri(uri) {
                        vocab_entry_resource_contents(
                            self.host.vocab_resource_value(),
                            &key,
                            request.uri.as_str(),
                        )?
                    } else if base_uri == SCHEMAS_URI {
                        require_resource_kind("schemas")?;
                        json_resource_contents_with_meta(
                            self.host.schemas_resource_value(),
                            request.uri.clone(),
                            Some(resource_meta(
                                "schemas",
                                Some(schema_resource_uri("schemas")),
                                None,
                            )),
                        )?
                    } else if base_uri == TOOL_SCHEMAS_URI {
                        require_resource_kind("tool-schemas")?;
                        json_resource_contents_with_meta(
                            self.host.tool_schemas_resource_value(),
                            request.uri.clone(),
                            Some(resource_meta(
                                "tool-schemas",
                                Some(schema_resource_uri("tool-schemas")),
                                None,
                            )),
                        )?
                    } else if base_uri == SELF_DESCRIPTION_AUDIT_URI {
                        require_resource_kind("self-description-audit")?;
                        self_description_audit_resource_contents(
                            self.host
                                .capabilities_resource_value()
                                .map_err(map_query_error)?,
                            request.uri.as_str(),
                        )?
                    } else if base_uri == SESSION_URI {
                        json_resource_contents_with_meta(
                            self.host
                                .session_resource_value(self.session.as_ref())
                                .map_err(map_query_error)?,
                            request.uri.clone(),
                            Some(resource_meta(
                                "session",
                                Some(schema_resource_uri("session")),
                                None,
                            )),
                        )?
                    } else if base_uri == PLANS_URI {
                        json_resource_contents_with_meta(
                            self.host
                                .plans_resource_value(Arc::clone(&self.session), uri)
                                .map_err(map_query_error)?,
                            request.uri.clone(),
                            Some(resource_meta(
                                "plans",
                                Some(schema_resource_uri("plans")),
                                None,
                            )),
                        )?
                    } else if base_uri == CONTRACTS_URI {
                        require_resource_kind("contracts")?;
                        json_resource_contents_with_meta(
                            self.host
                                .contracts_resource_value(Arc::clone(&self.session), uri)
                                .map_err(map_query_error)?,
                            request.uri.clone(),
                            Some(resource_meta(
                                "contracts",
                                Some(schema_resource_uri("contracts")),
                                None,
                            )),
                        )?
                    } else if base_uri == ENTRYPOINTS_URI {
                        require_resource_kind("entrypoints")?;
                        json_resource_contents_with_meta(
                            self.host
                                .entrypoints_resource_value(Arc::clone(&self.session), uri)
                                .map_err(map_query_error)?,
                            request.uri.clone(),
                            Some(resource_meta(
                                "entrypoints",
                                Some(schema_resource_uri("entrypoints")),
                                None,
                            )),
                        )?
                    } else if let Some(args) = parse_file_resource_uri(uri)? {
                        require_resource_kind("file")?;
                        json_resource_contents_with_meta(
                            self.host
                                .file_resource_value(self.session.as_ref(), uri, args)
                                .map_err(map_query_error)?,
                            request.uri.clone(),
                            Some(resource_meta(
                                "file",
                                Some(schema_resource_uri("file")),
                                None,
                            )),
                        )?
                    } else if let Some(query) = parse_search_resource_uri(uri) {
                        require_resource_kind("search")?;
                        json_resource_contents_with_meta(
                            self.host
                                .search_resource_value(Arc::clone(&self.session), uri, &query)
                                .map_err(map_query_error)?,
                            request.uri.clone(),
                            Some(resource_meta(
                                "search",
                                Some(schema_resource_uri("search")),
                                None,
                            )),
                        )?
                    } else if let Some(id) = parse_symbol_resource_uri(uri)? {
                        require_resource_kind("symbol")?;
                        json_resource_contents_with_meta(
                            self.host
                                .symbol_resource_value(Arc::clone(&self.session), &id)
                                .map_err(map_query_error)?,
                            request.uri.clone(),
                            Some(resource_meta(
                                "symbol",
                                Some(schema_resource_uri("symbol")),
                                None,
                            )),
                        )?
                    } else if let Some(lineage) = parse_lineage_resource_uri(uri) {
                        require_resource_kind("lineage")?;
                        json_resource_contents_with_meta(
                            self.host
                                .lineage_resource_value(self.session.as_ref(), uri, &lineage)
                                .map_err(map_query_error)?,
                            request.uri.clone(),
                            Some(resource_meta(
                                "lineage",
                                Some(schema_resource_uri("lineage")),
                                None,
                            )),
                        )?
                    } else if let Some(plan_id) = parse_plan_resource_uri(uri) {
                        json_resource_contents_with_meta(
                            self.host
                                .plan_resource_value(&plan_id)
                                .map_err(map_query_error)?,
                            request.uri.clone(),
                            Some(resource_meta(
                                "plan",
                                Some(schema_resource_uri("plan")),
                                None,
                            )),
                        )?
                    } else if let Some(task_id) = parse_task_resource_uri(uri) {
                        require_resource_kind("task")?;
                        json_resource_contents_with_meta(
                            self.host
                                .task_resource_value(self.session.as_ref(), uri, &task_id)
                                .map_err(map_query_error)?,
                            request.uri.clone(),
                            Some(resource_meta(
                                "task",
                                Some(schema_resource_uri("task")),
                                None,
                            )),
                        )?
                    } else if let Some(event_id) = parse_event_resource_uri(uri) {
                        require_resource_kind("event")?;
                        json_resource_contents_with_meta(
                            self.host
                                .event_resource_value(&event_id)
                                .map_err(map_query_error)?,
                            request.uri.clone(),
                            Some(resource_meta(
                                "event",
                                Some(schema_resource_uri("event")),
                                None,
                            )),
                        )?
                    } else if let Some(memory_id) = parse_memory_resource_uri(uri) {
                        require_resource_kind("memory")?;
                        json_resource_contents_with_meta(
                            self.host
                                .memory_resource_value(self.session.as_ref(), &memory_id)
                                .map_err(map_query_error)?,
                            request.uri.clone(),
                            Some(resource_meta(
                                "memory",
                                Some(schema_resource_uri("memory")),
                                None,
                            )),
                        )?
                    } else if let Some(edge_id) = parse_edge_resource_uri(uri) {
                        require_resource_kind("edge")?;
                        json_resource_contents_with_meta(
                            self.host
                                .edge_resource_value(self.session.as_ref(), &edge_id)
                                .map_err(map_query_error)?,
                            request.uri.clone(),
                            Some(resource_meta(
                                "edge",
                                Some(schema_resource_uri("edge")),
                                None,
                            )),
                        )?
                    } else if let Some((tool_name, action, tag)) =
                        parse_tool_variant_schema_resource_uri(uri)
                    {
                        require_tool_action_name(&tool_name, &action)?;
                        tool_variant_schema_resource_contents(&tool_name, &action, &tag, uri)?
                    } else if let Some((tool_name, action, tag)) =
                        parse_tool_variant_example_resource_uri(uri)
                    {
                        require_tool_examples()?;
                        require_tool_action_name(&tool_name, &action)?;
                        tool_variant_example_resource_contents(&tool_name, &action, &tag, uri)?
                    } else if let Some((tool_name, action, tag)) =
                        parse_tool_variant_shape_resource_uri(uri)
                    {
                        require_tool_examples()?;
                        require_tool_action_name(&tool_name, &action)?;
                        tool_variant_shape_resource_contents(&tool_name, &action, &tag, uri)?
                    } else if let Some((tool_name, action, tag)) =
                        parse_tool_variant_recipe_resource_uri(uri)
                    {
                        require_tool_examples()?;
                        require_tool_action_name(&tool_name, &action)?;
                        tool_recipe_resource_contents(&tool_name, &action, Some(&tag), uri)?
                    } else if let Some((tool_name, action)) =
                        parse_tool_action_example_resource_uri(uri)
                    {
                        require_tool_examples()?;
                        require_tool_action_name(&tool_name, &action)?;
                        tool_action_example_resource_contents(&tool_name, &action, uri)?
                    } else if let Some((tool_name, action)) =
                        parse_tool_action_shape_resource_uri(uri)
                    {
                        require_tool_examples()?;
                        require_tool_action_name(&tool_name, &action)?;
                        tool_action_shape_resource_contents(&tool_name, &action, uri)?
                    } else if let Some((tool_name, action)) =
                        parse_tool_action_recipe_resource_uri(uri)
                    {
                        require_tool_examples()?;
                        require_tool_action_name(&tool_name, &action)?;
                        tool_recipe_resource_contents(&tool_name, &action, None, uri)?
                    } else if let Some(tool_name) = parse_tool_example_resource_uri(uri) {
                        require_tool_examples()?;
                        require_tool_name(&tool_name)?;
                        tool_example_resource_contents(&tool_name, uri)?
                    } else if let Some(tool_name) = parse_tool_shape_resource_uri(uri) {
                        require_tool_examples()?;
                        require_tool_name(&tool_name)?;
                        tool_shape_resource_contents(&tool_name, uri)?
                    } else if let Some(resource_kind) = parse_resource_example_resource_uri(uri) {
                        require_resource_examples()?;
                        require_resource_kind(&resource_kind)?;
                        resource_example_resource_contents(&resource_kind, uri)?
                    } else if let Some(resource_kind) = parse_resource_shape_resource_uri(uri) {
                        require_resource_examples()?;
                        require_resource_kind(&resource_kind)?;
                        resource_shape_resource_contents(&resource_kind, uri)?
                    } else if let Some((tool_name, action)) =
                        parse_tool_action_schema_resource_uri(uri)
                    {
                        require_tool_action_name(&tool_name, &action)?;
                        tool_action_schema_resource_contents(&tool_name, &action, uri)?
                    } else if let Some(tool_name) = parse_tool_schema_resource_uri(uri) {
                        require_tool_name(&tool_name)?;
                        tool_schema_resource_contents_with_features(
                            &tool_name,
                            uri,
                            &self.host.features,
                        )?
                    } else if let Some(resource_kind) = parse_schema_resource_uri(uri) {
                        require_resource_kind(&resource_kind)?;
                        match resource_kind.as_str() {
                    "capabilities" => schema_resource_contents::<CapabilitiesResourcePayload>(
                        uri,
                        "PRISM Capabilities Resource Schema",
                        "JSON Schema for the canonical PRISM capabilities resource payload.",
                        "capabilities",
                    )?,
                    "session" => schema_resource_contents::<SessionResourcePayload>(
                        uri,
                        "PRISM Session Resource Schema",
                        "JSON Schema for the PRISM session resource payload.",
                        "session",
                    )?,
                    "protected-state" => schema_resource_contents::<ProtectedStateResourcePayload>(
                        uri,
                        "PRISM Protected State Resource Schema",
                        "JSON Schema for protected .prism stream verification status, trust diagnostics, and repair guidance.",
                        "protected-state",
                    )?,
                    "vocab" => schema_resource_contents::<VocabularyResourcePayload>(
                        uri,
                        "PRISM Vocabulary Resource Schema",
                        "JSON Schema for the canonical PRISM vocabulary resource payload.",
                        "vocab",
                    )?,
                    "plans" => schema_resource_contents::<PlansResourcePayload>(
                        uri,
                        "PRISM Plans Resource Schema",
                        "JSON Schema for the PRISM plans discovery resource payload.",
                        "plans",
                    )?,
                    "plan" => schema_resource_contents::<PlanResourcePayload>(
                        uri,
                        "PRISM Plan Resource Schema",
                        "JSON Schema for the PRISM plan detail resource payload.",
                        "plan",
                    )?,
                    "contracts" => schema_resource_contents::<ContractsResourcePayload>(
                        uri,
                        "PRISM Contracts Resource Schema",
                        "JSON Schema for the PRISM contracts discovery resource payload.",
                        "contracts",
                    )?,
                    "schemas" => schema_resource_contents::<ResourceSchemaCatalogPayload>(
                        uri,
                        "PRISM Resource Schema Catalog Schema",
                        "JSON Schema for the PRISM resource schema catalog payload.",
                        "schemas",
                    )?,
                    "tool-schemas" => schema_resource_contents::<ToolSchemaCatalogPayload>(
                        uri,
                        "PRISM Tool Schema Catalog Schema",
                        "JSON Schema for the PRISM MCP tool schema catalog payload.",
                        "tool-schemas",
                    )?,
                    "entrypoints" => schema_resource_contents::<EntrypointsResourcePayload>(
                        uri,
                        "PRISM Entrypoints Resource Schema",
                        "JSON Schema for the PRISM entrypoints resource payload.",
                        "entrypoints",
                    )?,
                    "search" => schema_resource_contents::<SearchResourcePayload>(
                        uri,
                        "PRISM Search Resource Schema",
                        "JSON Schema for the PRISM search resource payload.",
                        "search",
                    )?,
                    "file" => schema_resource_contents::<FileResourcePayload>(
                        uri,
                        "PRISM File Resource Schema",
                        "JSON Schema for read-only workspace file excerpt resources.",
                        "file",
                    )?,
                    "symbol" => schema_resource_contents::<SymbolResourcePayload>(
                        uri,
                        "PRISM Symbol Resource Schema",
                        "JSON Schema for the PRISM symbol resource payload.",
                        "symbol",
                    )?,
                    "lineage" => schema_resource_contents::<LineageResourcePayload>(
                        uri,
                        "PRISM Lineage Resource Schema",
                        "JSON Schema for the PRISM lineage resource payload.",
                        "lineage",
                    )?,
                    "task" => schema_resource_contents::<TaskResourcePayload>(
                        uri,
                        "PRISM Task Resource Schema",
                        "JSON Schema for the PRISM task replay resource payload.",
                        "task",
                    )?,
                    "event" => schema_resource_contents::<EventResourcePayload>(
                        uri,
                        "PRISM Event Resource Schema",
                        "JSON Schema for the PRISM event resource payload.",
                        "event",
                    )?,
                    "memory" => schema_resource_contents::<MemoryResourcePayload>(
                        uri,
                        "PRISM Memory Resource Schema",
                        "JSON Schema for the PRISM memory resource payload.",
                        "memory",
                    )?,
                    "edge" => schema_resource_contents::<EdgeResourcePayload>(
                        uri,
                        "PRISM Edge Resource Schema",
                        "JSON Schema for the PRISM inferred-edge resource payload.",
                        "edge",
                    )?,
                    "tool-shape" => schema_resource_contents::<ToolShapeResourcePayload>(
                        uri,
                        "PRISM Tool Shape Resource Schema",
                        "JSON Schema for compact tool shape companion resources.",
                        "tool-shape",
                    )?,
                    "tool-example" => schema_resource_contents::<ToolExampleResourcePayload>(
                        uri,
                        "PRISM Tool Example Resource Schema",
                        "JSON Schema for compact tool example companion resources.",
                        "tool-example",
                    )?,
                    "resource-shape" => schema_resource_contents::<ResourceShapeResourcePayload>(
                        uri,
                        "PRISM Resource Shape Resource Schema",
                        "JSON Schema for compact resource shape companion resources.",
                        "resource-shape",
                    )?,
                    "resource-example" => schema_resource_contents::<ResourceExampleResourcePayload>(
                        uri,
                        "PRISM Resource Example Resource Schema",
                        "JSON Schema for compact resource example companion resources.",
                        "resource-example",
                    )?,
                    "capabilities-section" => schema_resource_contents::<CapabilitiesSectionResourcePayload>(
                        uri,
                        "PRISM Capabilities Section Resource Schema",
                        "JSON Schema for segmented capabilities section resources.",
                        "capabilities-section",
                    )?,
                    "vocab-entry" => schema_resource_contents::<VocabularyEntryResourcePayload>(
                        uri,
                        "PRISM Vocabulary Entry Resource Schema",
                        "JSON Schema for segmented vocabulary entry resources.",
                        "vocab-entry",
                    )?,
                    "self-description-audit" => schema_resource_contents::<SelfDescriptionAuditPayload>(
                        uri,
                        "PRISM Self-Description Audit Resource Schema",
                        "JSON Schema for the self-description audit resource.",
                        "self-description-audit",
                    )?,
                    _ => {
                        return Err(McpError::resource_not_found(
                            "resource_not_found",
                            Some(json!({ "uri": request.uri })),
                        ))
                    }
                        }
                    } else {
                        return Err(McpError::resource_not_found(
                            "resource_not_found",
                            Some(json!({ "uri": request.uri })),
                        ));
                    })
                })()
            })
            .await;
        let contents = match contents_result {
            Ok(contents) => contents,
            Err(error) => {
                self.record_surface_call(
                    "resource_read",
                    base_uri,
                    uri.to_string(),
                    started_at,
                    started,
                    false,
                    Some(error.to_string()),
                    Some(json!({ "uri": uri, "baseUri": base_uri })),
                    None,
                    json!({ "uri": uri, "baseUri": base_uri }),
                    resource_trace.phases,
                );
                return Err(error);
            }
        };
        let result = ReadResourceResult::new(vec![contents]);
        self.record_surface_call(
            "resource_read",
            base_uri,
            uri.to_string(),
            started_at,
            started,
            true,
            None,
            Some(json!({ "uri": uri, "baseUri": base_uri })),
            Some(json!({
                "count": result.contents.len(),
                "uris": result
                    .contents
                    .iter()
                    .map(|content| Self::resource_content_uri(content).to_string())
                    .collect::<Vec<_>>(),
            })),
            json!({ "uri": uri, "baseUri": base_uri }),
            resource_trace.phases,
        );
        Ok(result)
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, McpError> {
        let started_at = current_timestamp();
        let started = Instant::now();
        let result = ListResourceTemplatesResult {
            next_cursor: None,
            resource_templates: crate::capabilities_resource::resource_template_capabilities(
                &self.host.features,
            )
            .into_iter()
            .map(|template| {
                let name = template.name.clone();
                RawResourceTemplate::new(template.uri_template, name.clone())
                    .with_description(template.description)
                    .with_mime_type(template.mime_type)
                    .with_title(name)
                    .no_annotation()
            })
            .collect(),
            meta: None,
        };
        self.record_surface_call(
            "resource_template_list",
            "list_resource_templates",
            "list resource templates".to_string(),
            started_at,
            started,
            true,
            None,
            Some(json!({ "method": "list_resource_templates" })),
            Some(json!({
                "count": result.resource_templates.len(),
                "uris": result
                    .resource_templates
                    .iter()
                    .take(8)
                    .map(|resource| resource.uri_template.clone())
                    .collect::<Vec<_>>(),
            })),
            json!({ "method": "list_resource_templates" }),
            Vec::new(),
        );
        Ok(result)
    }
}

impl PrismMcpServer {
    fn server_instructions(&self) -> String {
        crate::instructions::render_instructions_index_with_features(&self.host.features)
    }
}
