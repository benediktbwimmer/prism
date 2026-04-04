use prism_core::{AdmissionBusyError, AuthenticatedPrincipal, ObservedChangeFlushTrigger};
use prism_ir::{CredentialCapability, CredentialId};
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

use crate::dashboard_events::MutationRun;
use crate::*;

pub(crate) struct MutationDashboardMeta {
    task_id: Option<String>,
    result_ids: Vec<String>,
    violation_count: usize,
    publish_task_update: bool,
    publish_coordination_update: bool,
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

impl MutationDashboardMeta {
    pub(crate) fn task(
        task_id: Option<String>,
        result_ids: Vec<String>,
        violation_count: usize,
    ) -> Self {
        Self {
            task_id,
            result_ids,
            violation_count,
            publish_task_update: true,
            publish_coordination_update: false,
        }
    }

    pub(crate) fn coordination(result_ids: Vec<String>, violation_count: usize) -> Self {
        Self {
            task_id: None,
            result_ids,
            violation_count,
            publish_task_update: false,
            publish_coordination_update: true,
        }
    }
}

impl PrismMcpServer {
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
            self.host.dashboard_state().as_ref(),
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
        });
    }

    pub(crate) fn build_tool_router() -> ToolRouter<Self> {
        let _: fn(&Self, Parameters<PrismConceptArgs>) -> Result<CallToolResult, McpError> =
            Self::prism_concept;
        let _: fn(&Self, Parameters<PrismTaskBriefArgs>) -> Result<CallToolResult, McpError> =
            Self::prism_task_brief;
        Self::tool_router()
    }

    pub(crate) fn transport_bind_tool_schema(mut tool: Tool) -> Tool {
        if let Some(Value::Object(schema)) = tool_transport_input_schema_value(tool.name.as_ref()) {
            tool.input_schema = Arc::new(schema);
        }
        tool
    }

    fn authenticate_mutation(
        &self,
        credential: &PrismMutationCredentialArgs,
        requirement: MutationCapabilityRequirement,
    ) -> Result<AuthenticatedPrincipal, McpError> {
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
        workspace
            .bind_or_validate_worktree_principal(&authenticated)
            .map_err(|error| {
                McpError::invalid_params(
                    "prism_mutate principal conflicts with the worktree-bound principal",
                    Some(json!({
                        "code": "mutation_worktree_principal_conflict",
                        "worktreeId": error.worktree_id,
                        "boundPrincipal": {
                            "authorityId": error.bound_principal.authority_id,
                            "principalId": error.bound_principal.principal_id,
                            "name": error.bound_principal.principal_name,
                        },
                        "attemptedPrincipal": {
                            "authorityId": error.attempted_principal.authority_id,
                            "principalId": error.attempted_principal.principal_id,
                            "name": error.attempted_principal.principal_name,
                        },
                        "nextAction": "Use the same principal for this worktree, or move the other principal onto a separate git worktree before attempting authenticated mutations.",
                    })),
                )
            })?;
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
        Ok(authenticated)
    }

    fn authenticate_mutation_with_run(
        &self,
        run: &MutationRun,
        credential: &PrismMutationCredentialArgs,
        requirement: MutationCapabilityRequirement,
    ) -> Result<AuthenticatedPrincipal, McpError> {
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
                let bind_started = Instant::now();
                if let Err(error) = workspace.bind_or_validate_worktree_principal(&authenticated) {
                    let mapped = McpError::invalid_params(
                        "prism_mutate principal conflicts with the worktree-bound principal",
                        Some(json!({
                            "code": "mutation_worktree_principal_conflict",
                            "worktreeId": error.worktree_id,
                            "boundPrincipal": {
                                "authorityId": error.bound_principal.authority_id,
                                "principalId": error.bound_principal.principal_id,
                                "name": error.bound_principal.principal_name,
                            },
                            "attemptedPrincipal": {
                                "authorityId": error.attempted_principal.authority_id,
                                "principalId": error.attempted_principal.principal_id,
                                "name": error.attempted_principal.principal_name,
                            },
                            "nextAction": "Use the same principal for this worktree, or move the other principal onto a separate git worktree before attempting authenticated mutations.",
                        })),
                    );
                    run.record_phase(
                        "mutation.auth.bindWorktreePrincipal",
                        &json!({ "credentialId": credential.credential_id }),
                        bind_started.elapsed(),
                        false,
                        Some(mapped.to_string()),
                    );
                    return Err(mapped);
                }
                run.record_phase(
                    "mutation.auth.bindWorktreePrincipal",
                    &json!({ "credentialId": credential.credential_id }),
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
                Ok(authenticated)
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
        G: FnOnce(&T) -> MutationDashboardMeta,
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
        G: FnOnce(&T) -> MutationDashboardMeta,
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
        G: FnOnce(&T) -> MutationDashboardMeta,
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
                if meta.publish_task_update && self.host.ui_enabled() {
                    let publish_started = Instant::now();
                    match self
                        .host
                        .dashboard_task_snapshot(Some(self.session.as_ref()))
                    {
                        Ok(snapshot) => {
                            run.record_phase(
                                "mutation.publishTaskUpdate.buildSnapshot",
                                &json!({ "action": action }),
                                publish_started.elapsed(),
                                true,
                                None,
                            );
                            let encode_started = Instant::now();
                            match serde_json::to_value(&snapshot) {
                                Ok(snapshot_json) => {
                                    run.record_phase(
                                        "mutation.publishTaskUpdate.encode",
                                        &json!({ "action": action }),
                                        encode_started.elapsed(),
                                        true,
                                        None,
                                    );
                                    let event_started = Instant::now();
                                    self.host
                                        .dashboard_state()
                                        .publish_value("task.updated", snapshot_json);
                                    run.record_phase(
                                        "mutation.publishTaskUpdate.publishEvent",
                                        &json!({ "action": action }),
                                        event_started.elapsed(),
                                        true,
                                        None,
                                    );
                                    run.record_phase(
                                        "mutation.publishTaskUpdate",
                                        &json!({ "action": action }),
                                        publish_started.elapsed(),
                                        true,
                                        None,
                                    );
                                }
                                Err(error) => {
                                    let message = error.to_string();
                                    run.record_phase(
                                        "mutation.publishTaskUpdate.encode",
                                        &json!({ "action": action }),
                                        encode_started.elapsed(),
                                        false,
                                        Some(message.clone()),
                                    );
                                    run.record_phase(
                                        "mutation.publishTaskUpdate",
                                        &json!({ "action": action }),
                                        publish_started.elapsed(),
                                        false,
                                        Some(message),
                                    );
                                }
                            }
                        }
                        Err(error) => {
                            let message = error.to_string();
                            run.record_phase(
                                "mutation.publishTaskUpdate.buildSnapshot",
                                &json!({ "action": action }),
                                publish_started.elapsed(),
                                false,
                                Some(message.clone()),
                            );
                            run.record_phase(
                                "mutation.publishTaskUpdate",
                                &json!({ "action": action }),
                                publish_started.elapsed(),
                                false,
                                Some(message),
                            );
                        }
                    }
                }
                if meta.publish_coordination_update && self.host.ui_enabled() {
                    let publish_started = Instant::now();
                    let _ = self.host.publish_dashboard_coordination_update();
                    run.record_phase(
                        "mutation.publishCoordinationUpdate",
                        &json!({ "action": action }),
                        publish_started.elapsed(),
                        true,
                        None,
                    );
                }
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
        description = "Execute a read-only TypeScript query against the live PRISM graph. Read prism://api-reference for the available prism API.",
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
        let credential = args.credential;
        match args.mutation {
            PrismMutationKindArgs::DeclareWork(args) => {
                let authenticated = self.authenticate_mutation(
                    &credential,
                    MutationCapabilityRequirement::AnyAuthenticated,
                )?;
                let result = self.execute_logged_mutation(
                    "mutate.declare_work",
                    MutationRefreshPolicy::None,
                    || {
                        self.host.declare_work_without_refresh_authenticated(
                            self.session.as_ref(),
                            args,
                            Some(&authenticated),
                        )
                    },
                    |result| {
                        MutationDashboardMeta::task(
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
                    &credential,
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
                            Some(&authenticated),
                        )
                    },
                    |result| {
                        let mut result_ids = result.event_ids.clone();
                        result_ids.push(result.task_id.clone());
                        MutationDashboardMeta::task(Some(result.task_id.clone()), result_ids, 0)
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
                    &credential,
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
                            Some(&authenticated),
                        )
                    },
                    |result| {
                        MutationDashboardMeta::task(
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
                    &credential,
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
                            Some(&authenticated),
                        )
                    },
                    |result| {
                        MutationDashboardMeta::task(
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
                    &credential,
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
                            Some(&authenticated),
                        )
                    },
                    |result| {
                        MutationDashboardMeta::task(
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
                    &credential,
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
                            Some(&authenticated),
                        )
                    },
                    |result| {
                        MutationDashboardMeta::task(
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
                    &credential,
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
                            Some(&authenticated),
                        )
                    },
                    |result| {
                        MutationDashboardMeta::task(
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
                    &credential,
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
                                Some(&authenticated),
                            )
                    },
                    |result| {
                        MutationDashboardMeta::task(
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
                    &credential,
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
                        MutationDashboardMeta::task(
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
                    &credential,
                    MutationCapabilityRequirement::MutateRepoMemory,
                )?;
                self.require_declared_work_context("infer_edge", args.task_id.is_some())?;
                let result = self.execute_logged_mutation(
                    "mutate.infer_edge",
                    MutationRefreshPolicy::None,
                    || self.host.store_inferred_edge(self.session.as_ref(), args),
                    |result| {
                        MutationDashboardMeta::task(
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
                    &credential,
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
                            Some(&authenticated),
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
                        MutationDashboardMeta::coordination(result_ids, result.violations.len())
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
                    &credential,
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
                    args.task_id.is_some(),
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
                            Some(&authenticated),
                        )
                    },
                    |result| {
                        MutationDashboardMeta::coordination(
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
                    &credential,
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
                            Some(&authenticated),
                        )
                    },
                    |result| {
                        let mut result_ids = result.event_ids.clone();
                        if let Some(claim_id) = &result.claim_id {
                            result_ids.push(claim_id.clone());
                        }
                        MutationDashboardMeta::coordination(result_ids, result.violations.len())
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
                    &credential,
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
                            Some(&authenticated),
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
                        MutationDashboardMeta::coordination(result_ids, result.violations.len())
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
                    &credential,
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
                            Some(&authenticated),
                        )
                    },
                    |result| {
                        MutationDashboardMeta::task(
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
                    &credential,
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
                            Some(&authenticated),
                        )
                    },
                    |result| {
                        MutationDashboardMeta::task(
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
                    &credential,
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
                            Some(&authenticated),
                        )
                    },
                    |result| {
                        MutationDashboardMeta::task(
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
                    &credential,
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
                        MutationDashboardMeta::coordination(result_ids, 0)
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
                    &credential,
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
                            Some(&authenticated),
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
                        MutationDashboardMeta::coordination(result_ids, 0)
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
                    &credential,
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
                            Some(&authenticated),
                        )
                    },
                    |result| {
                        let mut result_ids = vec![result.job_id.clone()];
                        if let Some(concept_handle) = &result.concept_handle {
                            result_ids.push(concept_handle.clone());
                        }
                        MutationDashboardMeta::coordination(result_ids, 0)
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
                    &credential,
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
                            Some(&authenticated),
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
                        MutationDashboardMeta::coordination(result_ids, 0)
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
                    &credential,
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
                            Some(&authenticated),
                        )
                    },
                    |result| MutationDashboardMeta::coordination(vec![result.job_id.clone()], 0),
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
                .map(Self::transport_bind_tool_schema)
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
                    .map(Self::transport_bind_tool_schema)
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
            instruction_set_resource_links()
                .into_iter()
                .map(|resource| resource.no_annotation()),
        );
        resources.extend([
            RawResource::new(API_REFERENCE_URI, "PRISM API Reference")
                .with_description(
                    "TypeScript query surface, d.ts-style contract, and usage recipes",
                )
                .with_mime_type("text/markdown")
                .with_title("PRISM API Reference")
                .no_annotation(),
            capabilities_resource_link()
                .with_title("PRISM Capabilities")
                .no_annotation(),
            protected_state_resource_link()
                .with_title("PRISM Protected State")
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
            contracts_resource_link()
                .with_title("PRISM Contracts")
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
        let (contents_result, resource_trace) =
            crate::resource_trace::ResourceTraceState::scope(async {
                (|| -> Result<ResourceContents, McpError> {
                    Ok(if let Some(instruction_set_id) =
                        crate::instructions::parse_instruction_resource_uri(uri)
                    {
                        let markdown = match instruction_set_id {
                            None => self.server_instructions(),
                            Some(id) => crate::instructions::render_instruction_set(
                                &id,
                                self.host.features.mode_label(),
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
                    } else if base_uri == PROTECTED_STATE_URI {
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
                        json_resource_contents_with_meta(
                            self.host.vocab_resource_value(),
                            request.uri.clone(),
                            Some(resource_meta(
                                "vocab",
                                Some(schema_resource_uri("vocab")),
                                None,
                            )),
                        )?
                    } else if base_uri == SCHEMAS_URI {
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
                        json_resource_contents_with_meta(
                            self.host.tool_schemas_resource_value(),
                            request.uri.clone(),
                            Some(resource_meta(
                                "tool-schemas",
                                Some(schema_resource_uri("tool-schemas")),
                                None,
                            )),
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
                    } else if let Some((tool_name, action)) =
                        parse_tool_action_schema_resource_uri(uri)
                    {
                        tool_action_schema_resource_contents(&tool_name, &action, uri)?
                    } else if let Some(tool_name) = parse_tool_schema_resource_uri(uri) {
                        tool_schema_resource_contents(&tool_name, uri)?
                    } else if let Some(resource_kind) = parse_schema_resource_uri(uri) {
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
            resource_templates: vec![
                RawResourceTemplate::new(
                    ENTRYPOINTS_RESOURCE_TEMPLATE_URI,
                    "PRISM Entrypoints Page",
                )
                .with_description(
                    "Read workspace entrypoints with optional `limit` and opaque `cursor` pagination",
                )
                .with_mime_type("application/json")
                .no_annotation(),
                RawResourceTemplate::new(PLANS_RESOURCE_TEMPLATE_URI, "PRISM Plans Page")
                    .with_description(
                        "Read structured plan discovery results with optional `status`, `scope`, `contains`, `limit`, and opaque `cursor` pagination",
                    )
                    .with_mime_type("application/json")
                    .with_title("PRISM Plans Page")
                    .no_annotation(),
                RawResourceTemplate::new(PLAN_RESOURCE_TEMPLATE_URI, "PRISM Plan")
                    .with_description("Read a coordination plan by id")
                    .with_mime_type("application/json")
                    .with_title("PRISM Plan")
                    .no_annotation(),
                RawResourceTemplate::new(SCHEMA_RESOURCE_TEMPLATE_URI, "PRISM Resource Schema")
                    .with_description(
                        "Read a JSON Schema document for a structured PRISM resource payload kind such as `session`, `plans`, `plan`, `entrypoints`, `search`, `symbol`, `lineage`, `task`, `event`, `memory`, or `edge`",
                    )
                    .with_mime_type("application/schema+json")
                    .with_title("PRISM Resource Schema")
                    .no_annotation(),
                RawResourceTemplate::new(TOOL_SCHEMA_RESOURCE_TEMPLATE_URI, "PRISM Tool Schema")
                    .with_description(
                        "Read a JSON Schema document for a PRISM MCP tool input payload such as `prism_query` or `prism_mutate`",
                    )
                    .with_mime_type("application/schema+json")
                    .with_title("PRISM Tool Schema")
                    .no_annotation(),
                RawResourceTemplate::new(
                    TOOL_ACTION_SCHEMA_RESOURCE_TEMPLATE_URI,
                    "PRISM Tool Action Schema",
                )
                .with_description(
                    "Read an exact JSON Schema document for one tagged PRISM MCP tool action such as `prism_mutate` action `coordination`",
                )
                .with_mime_type("application/schema+json")
                .with_title("PRISM Tool Action Schema")
                .no_annotation(),
                RawResourceTemplate::new(SEARCH_RESOURCE_TEMPLATE_URI, "PRISM Search")
                    .with_description(
                        "Read structured PRISM search results and diagnostics for a query string with optional `limit`, `cursor`, `strategy`, `ownerKind`, `kind`, `path`, `module`, `taskId`, and `includeInferred` options",
                    )
                    .with_mime_type("application/json")
                    .no_annotation(),
                RawResourceTemplate::new(FILE_RESOURCE_TEMPLATE_URI, "PRISM File")
                    .with_description(
                        "Read a workspace file excerpt by path with optional `startLine`, `endLine`, and `maxChars` narrowing",
                    )
                    .with_mime_type("application/json")
                    .no_annotation(),
                RawResourceTemplate::new(SYMBOL_RESOURCE_TEMPLATE_URI, "PRISM Symbol Snapshot")
                    .with_description(
                        "Read a structured snapshot for an exact symbol, including relations, lineage, validation recipe, blast radius, and related failures",
                    )
                    .with_mime_type("application/json")
                    .no_annotation(),
                RawResourceTemplate::new(LINEAGE_RESOURCE_TEMPLATE_URI, "PRISM Lineage")
                    .with_description(
                        "Read structured lineage history, current nodes, and status for a lineage id with paged history",
                    )
                    .with_mime_type("application/json")
                    .no_annotation(),
                RawResourceTemplate::new(TASK_RESOURCE_TEMPLATE_URI, "PRISM Task Replay")
                    .with_description(
                        "Read the outcome-event timeline recorded for a specific task context with optional `limit` and opaque `cursor` pagination",
                    )
                    .with_mime_type("application/json")
                    .no_annotation(),
                RawResourceTemplate::new(EVENT_RESOURCE_TEMPLATE_URI, "PRISM Event")
                    .with_description("Read a single recorded outcome event by id")
                    .with_mime_type("application/json")
                    .no_annotation(),
                RawResourceTemplate::new(MEMORY_RESOURCE_TEMPLATE_URI, "PRISM Memory")
                    .with_description("Read a single episodic memory entry by id")
                    .with_mime_type("application/json")
                    .no_annotation(),
                RawResourceTemplate::new(EDGE_RESOURCE_TEMPLATE_URI, "PRISM Inferred Edge")
                    .with_description(
                        "Read a single inferred-edge record, including scope, task, and evidence",
                    )
                    .with_mime_type("application/json")
                    .no_annotation(),
            ],
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
        crate::instructions::render_instructions_index(self.host.features.mode_label())
    }
}
