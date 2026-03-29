use prism_js::{
    AgentConceptResultView, AgentExpandResultView, AgentGatherResultView, AgentLocateResultView,
    AgentOpenResultView, AgentWorksetResultView,
};
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    service::RequestContext,
    tool, tool_router, ErrorData as McpError, RoleServer, ServerHandler,
};
use serde::Serialize;
use serde_json::json;
use std::sync::Arc;
use std::time::Instant;

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
    PersistedOnly,
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
    pub(crate) fn build_tool_router() -> ToolRouter<Self> {
        let _: fn(&Self, Parameters<PrismConceptArgs>) -> Result<CallToolResult, McpError> =
            Self::prism_concept;
        let _: fn(&Self, Parameters<PrismTaskBriefArgs>) -> Result<CallToolResult, McpError> =
            Self::prism_task_brief;
        Self::tool_router()
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
        let run = self.host.begin_mutation_run(self.session.as_ref(), action);
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
                    Ok(refresh) => run.record_phase(
                        "mutation.refreshWorkspace",
                        &json!({
                            "refreshPath": refresh.refresh_path,
                            "deferred": refresh.deferred,
                            "episodicReloaded": refresh.episodic_reloaded,
                            "inferenceReloaded": refresh.inference_reloaded,
                            "coordinationReloaded": refresh.coordination_reloaded,
                        }),
                        refresh_started.elapsed(),
                        true,
                        None,
                    ),
                    Err(error) => {
                        let message = error.to_string();
                        run.record_phase(
                            "mutation.refreshWorkspace",
                            &json!({ "refreshPath": "error" }),
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

        let operation_started = Instant::now();
        match operation() {
            Ok(result) => {
                run.record_phase(
                    "mutation.operation",
                    &json!({ "action": action }),
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
                        run.finish_error(mapped.to_string());
                        return Err(mapped);
                    }
                };
                if meta.publish_task_update {
                    let publish_started = Instant::now();
                    let _ = self
                        .host
                        .publish_dashboard_task_update(self.session.as_ref());
                    run.record_phase(
                        "mutation.publishTaskUpdate",
                        &json!({ "action": action }),
                        publish_started.elapsed(),
                        true,
                        None,
                    );
                }
                if meta.publish_coordination_update {
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
                run.finish_error(message);
                Err(map_query_error(error))
            }
        }
    }
}

#[tool_router]
impl PrismMcpServer {
    #[tool(
        description = "Mutate task or session context for subsequent mutations. Read prism://session to inspect current state.",
        annotations(
            title = "Mutate PRISM Session",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        ),
        output_schema =
            rmcp::handler::server::tool::schema_for_output::<PrismSessionMutationResult>()
                .unwrap()
    )]
    fn prism_session(
        &self,
        Parameters(args): Parameters<PrismSessionArgs>,
    ) -> Result<CallToolResult, McpError> {
        match args {
            PrismSessionArgs::StartTask(args) => {
                let description = args
                    .description
                    .as_deref()
                    .map(str::trim)
                    .filter(|description| !description.is_empty())
                    .map(ToOwned::to_owned);
                if description.is_none() && args.coordination_task_id.is_none() {
                    return Err(McpError::invalid_params(
                        "task description cannot be empty unless coordinationTaskId is provided",
                        Some(json!({ "field": "input.description" })),
                    ));
                }

                let task = self.execute_logged_mutation(
                    "session.start_task",
                    MutationRefreshPolicy::None,
                    || {
                        self.host.start_task(
                            self.session.as_ref(),
                            description.clone(),
                            args.tags.unwrap_or_default(),
                            args.coordination_task_id.clone(),
                        )
                    },
                    |task| {
                        let task_id = task.0.to_string();
                        MutationDashboardMeta::task(Some(task_id.clone()), vec![task_id], 0)
                    },
                )?;
                let task_id = task.0.to_string();
                let session = self
                    .host
                    .session_view_without_refresh(self.session.as_ref());
                structured_tool_result_with_links(
                    PrismSessionMutationResult {
                        action: SessionMutationActionSchema::StartTask,
                        task_id: Some(task_id.clone()),
                        event_id: None,
                        memory_id: None,
                        journal: None,
                        session,
                    },
                    vec![session_resource_link(), task_resource_link(&task_id)],
                )
            }
            PrismSessionArgs::BindCoordinationTask(args) => {
                if args.coordination_task_id.trim().is_empty() {
                    return Err(McpError::invalid_params(
                        "coordinationTaskId cannot be empty",
                        Some(json!({ "field": "input.coordinationTaskId" })),
                    ));
                }

                let session = self.execute_logged_mutation(
                    "session.bind_coordination_task",
                    MutationRefreshPolicy::PersistedOnly,
                    || {
                        self.host.configure_session_without_refresh(
                            self.session.as_ref(),
                            PrismConfigureSessionArgs {
                                limits: None,
                                current_task_id: None,
                                coordination_task_id: Some(args.coordination_task_id.clone()),
                                current_task_description: args.description.clone(),
                                current_task_tags: args.tags.clone(),
                                clear_current_task: None,
                                current_agent: None,
                                clear_current_agent: None,
                            },
                        )
                    },
                    |session| {
                        MutationDashboardMeta::task(
                            session
                                .current_task
                                .as_ref()
                                .map(|task| task.task_id.clone()),
                            session
                                .current_task
                                .as_ref()
                                .map(|task| vec![task.task_id.clone()])
                                .unwrap_or_default(),
                            0,
                        )
                    },
                )?;
                let mut links = vec![session_resource_link()];
                if let Some(task) = &session.current_task {
                    links.push(task_resource_link(&task.task_id));
                }
                structured_tool_result_with_links(
                    PrismSessionMutationResult {
                        action: SessionMutationActionSchema::BindCoordinationTask,
                        task_id: session
                            .current_task
                            .as_ref()
                            .map(|task| task.task_id.clone()),
                        event_id: None,
                        memory_id: None,
                        journal: None,
                        session,
                    },
                    links,
                )
            }
            PrismSessionArgs::Configure(args) => {
                let session = self.execute_logged_mutation(
                    "session.configure",
                    MutationRefreshPolicy::PersistedOnly,
                    || {
                        self.host
                            .configure_session_without_refresh(self.session.as_ref(), args)
                    },
                    |session| {
                        MutationDashboardMeta::task(
                            session
                                .current_task
                                .as_ref()
                                .map(|task| task.task_id.clone()),
                            session
                                .current_task
                                .as_ref()
                                .map(|task| vec![task.task_id.clone()])
                                .unwrap_or_default(),
                            0,
                        )
                    },
                )?;
                let mut links = vec![session_resource_link()];
                if let Some(task) = &session.current_task {
                    links.push(task_resource_link(&task.task_id));
                }
                structured_tool_result_with_links(
                    PrismSessionMutationResult {
                        action: SessionMutationActionSchema::Configure,
                        task_id: session
                            .current_task
                            .as_ref()
                            .map(|task| task.task_id.clone()),
                        event_id: None,
                        memory_id: None,
                        journal: None,
                        session,
                    },
                    links,
                )
            }
            PrismSessionArgs::FinishTask(args) => {
                if args.summary.trim().is_empty() {
                    return Err(McpError::invalid_params(
                        "task summary cannot be empty",
                        Some(json!({ "field": "input.summary" })),
                    ));
                }

                let result = self.execute_logged_mutation(
                    "session.finish_task",
                    MutationRefreshPolicy::PersistedOnly,
                    || {
                        self.host
                            .finish_task_without_refresh(self.session.as_ref(), args)
                    },
                    |result| {
                        MutationDashboardMeta::task(
                            Some(result.task_id.clone()),
                            vec![
                                result.task_id.clone(),
                                result.event_id.clone(),
                                result.memory_id.clone(),
                            ],
                            0,
                        )
                    },
                )?;
                let session = self
                    .host
                    .session_view_without_refresh(self.session.as_ref());
                structured_tool_result_with_links(
                    PrismSessionMutationResult {
                        action: SessionMutationActionSchema::FinishTask,
                        task_id: Some(result.task_id.clone()),
                        event_id: Some(result.event_id.clone()),
                        memory_id: Some(result.memory_id.clone()),
                        journal: Some(result.journal),
                        session,
                    },
                    vec![
                        session_resource_link(),
                        task_resource_link(&result.task_id),
                        event_resource_link(&result.event_id),
                        memory_resource_link(&result.memory_id),
                    ],
                )
            }
            PrismSessionArgs::AbandonTask(args) => {
                if args.summary.trim().is_empty() {
                    return Err(McpError::invalid_params(
                        "task summary cannot be empty",
                        Some(json!({ "field": "input.summary" })),
                    ));
                }

                let result = self.execute_logged_mutation(
                    "session.abandon_task",
                    MutationRefreshPolicy::PersistedOnly,
                    || {
                        self.host
                            .abandon_task_without_refresh(self.session.as_ref(), args)
                    },
                    |result| {
                        MutationDashboardMeta::task(
                            Some(result.task_id.clone()),
                            vec![
                                result.task_id.clone(),
                                result.event_id.clone(),
                                result.memory_id.clone(),
                            ],
                            0,
                        )
                    },
                )?;
                let session = self
                    .host
                    .session_view_without_refresh(self.session.as_ref());
                structured_tool_result_with_links(
                    PrismSessionMutationResult {
                        action: SessionMutationActionSchema::AbandonTask,
                        task_id: Some(result.task_id.clone()),
                        event_id: Some(result.event_id.clone()),
                        memory_id: Some(result.memory_id.clone()),
                        journal: Some(result.journal),
                        session,
                    },
                    vec![
                        session_resource_link(),
                        task_resource_link(&result.task_id),
                        event_resource_link(&result.event_id),
                        memory_resource_link(&result.memory_id),
                    ],
                )
            }
        }
    }

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
        if has_path
            && matches!(
                args.mode,
                Some(PrismOpenModeInput::Focus | PrismOpenModeInput::Edit)
            )
        {
            return Err(McpError::invalid_params(
                "path-based prism_open currently supports only raw mode",
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
        match args {
            PrismMutationArgs::Outcome(args) => {
                let result = self.execute_logged_mutation(
                    "mutate.outcome",
                    MutationRefreshPolicy::PersistedOnly,
                    || {
                        self.host
                            .store_outcome_without_refresh(self.session.as_ref(), args)
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
            PrismMutationArgs::Memory(args) => {
                let result = self.execute_logged_mutation(
                    "mutate.memory",
                    MutationRefreshPolicy::PersistedOnly,
                    || {
                        self.host
                            .store_memory_without_refresh(self.session.as_ref(), args)
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
            PrismMutationArgs::Concept(args) => {
                let result = self.execute_logged_mutation(
                    "mutate.concept",
                    MutationRefreshPolicy::PersistedOnly,
                    || {
                        self.host
                            .store_concept_without_refresh(self.session.as_ref(), args)
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
            PrismMutationArgs::ConceptRelation(args) => {
                let result = self.execute_logged_mutation(
                    "mutate.concept_relation",
                    MutationRefreshPolicy::PersistedOnly,
                    || {
                        self.host
                            .store_concept_relation(self.session.as_ref(), args)
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
            PrismMutationArgs::ValidationFeedback(args) => {
                let result = self.execute_logged_mutation(
                    "mutate.validation_feedback",
                    MutationRefreshPolicy::PersistedOnly,
                    || {
                        self.host
                            .store_validation_feedback_without_refresh(self.session.as_ref(), args)
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
            PrismMutationArgs::InferEdge(args) => {
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
            PrismMutationArgs::Coordination(args) => {
                let result = self.execute_logged_mutation(
                    "mutate.coordination",
                    MutationRefreshPolicy::None,
                    || self.host.store_coordination(self.session.as_ref(), args),
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
            PrismMutationArgs::Claim(args) => {
                let result = self.execute_logged_mutation(
                    "mutate.claim",
                    MutationRefreshPolicy::None,
                    || self.host.store_claim(self.session.as_ref(), args),
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
            PrismMutationArgs::Artifact(args) => {
                let result = self.execute_logged_mutation(
                    "mutate.artifact",
                    MutationRefreshPolicy::None,
                    || self.host.store_artifact(self.session.as_ref(), args),
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
            PrismMutationArgs::TestRan(args) => {
                let summary = format!(
                    "test `{}` {}",
                    args.test,
                    if args.passed { "passed" } else { "failed" }
                );
                let result = self.execute_logged_mutation(
                    "mutate.test_ran",
                    MutationRefreshPolicy::PersistedOnly,
                    || {
                        self.host.store_outcome_without_refresh(
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
                                evidence: Some(vec![OutcomeEvidenceInput::Test {
                                    name: args.test,
                                    passed: args.passed,
                                }]),
                                task_id: args.task_id,
                            },
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
            PrismMutationArgs::FailureObserved(args) => {
                let evidence = args
                    .trace
                    .map(|trace| vec![OutcomeEvidenceInput::StackTrace { hash: trace }]);
                let result = self.execute_logged_mutation(
                    "mutate.failure_observed",
                    MutationRefreshPolicy::PersistedOnly,
                    || {
                        self.host.store_outcome_without_refresh(
                            self.session.as_ref(),
                            PrismOutcomeArgs {
                                kind: OutcomeKindInput::FailureObserved,
                                anchors: args.anchors,
                                summary: args.summary,
                                result: Some(OutcomeResultInput::Failure),
                                evidence,
                                task_id: args.task_id,
                            },
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
            PrismMutationArgs::FixValidated(args) => {
                let result = self.execute_logged_mutation(
                    "mutate.fix_validated",
                    MutationRefreshPolicy::PersistedOnly,
                    || {
                        self.host.store_outcome_without_refresh(
                            self.session.as_ref(),
                            PrismOutcomeArgs {
                                kind: OutcomeKindInput::FixValidated,
                                anchors: args.anchors,
                                summary: args.summary,
                                result: Some(OutcomeResultInput::Success),
                                evidence: None,
                                task_id: args.task_id,
                            },
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
            PrismMutationArgs::CuratorPromoteEdge(args) => {
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
            PrismMutationArgs::CuratorApplyProposal(args) => {
                let result = self.execute_logged_mutation(
                    "mutate.curator_apply_proposal",
                    MutationRefreshPolicy::None,
                    || {
                        self.host
                            .apply_curator_proposal(self.session.as_ref(), args)
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
            PrismMutationArgs::CuratorPromoteConcept(args) => {
                let result = self.execute_logged_mutation(
                    "mutate.curator_promote_concept",
                    MutationRefreshPolicy::None,
                    || {
                        self.host
                            .promote_curator_concept(self.session.as_ref(), args)
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
            PrismMutationArgs::CuratorPromoteMemory(args) => {
                let result = self.execute_logged_mutation(
                    "mutate.curator_promote_memory",
                    MutationRefreshPolicy::None,
                    || {
                        self.host
                            .promote_curator_memory(self.session.as_ref(), args)
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
            PrismMutationArgs::CuratorRejectProposal(args) => {
                let result = self.execute_logged_mutation(
                    "mutate.curator_reject_proposal",
                    MutationRefreshPolicy::None,
                    || {
                        self.host
                            .reject_curator_proposal(self.session.as_ref(), args)
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
        if !self.host.features.is_tool_enabled(request.name.as_ref()) {
            return Err(McpError::invalid_params(
                "tool not found",
                Some(json!({ "name": request.name })),
            ));
        }
        let context = rmcp::handler::server::tool::ToolCallContext::new(self, request, context);
        self.tool_router.call(context).await
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult {
            tools: self
                .tool_router
                .list_all()
                .into_iter()
                .filter(|tool| self.host.features.is_tool_enabled(&tool.name))
                .collect(),
            next_cursor: None,
            meta: None,
        })
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.host
            .features
            .is_tool_enabled(name)
            .then(|| self.tool_router.get(name).cloned())
            .flatten()
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult {
            resources: vec![
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
            ],
            next_cursor: None,
            meta: None,
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, McpError> {
        let uri = request.uri.as_str();
        let (base_uri, _) = split_resource_uri(uri);
        let contents = if base_uri == API_REFERENCE_URI {
            ResourceContents::text(self.host.api_reference_markdown(), request.uri.clone())
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
                "plans" => schema_resource_contents::<PlansResourcePayload>(
                    uri,
                    "PRISM Plans Resource Schema",
                    "JSON Schema for the PRISM plans discovery resource payload.",
                    "plans",
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
        };

        Ok(ReadResourceResult::new(vec![contents]))
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, McpError> {
        Ok(ListResourceTemplatesResult {
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
                RawResourceTemplate::new(SCHEMA_RESOURCE_TEMPLATE_URI, "PRISM Resource Schema")
                    .with_description(
                        "Read a JSON Schema document for a structured PRISM resource payload kind such as `session`, `plans`, `entrypoints`, `search`, `symbol`, `lineage`, `task`, `event`, `memory`, or `edge`",
                    )
                    .with_mime_type("application/schema+json")
                    .with_title("PRISM Resource Schema")
                    .no_annotation(),
                RawResourceTemplate::new(TOOL_SCHEMA_RESOURCE_TEMPLATE_URI, "PRISM Tool Schema")
                    .with_description(
                        "Read a JSON Schema document for a PRISM MCP tool input payload such as `prism_query`, `prism_session`, or `prism_mutate`",
                    )
                    .with_mime_type("application/schema+json")
                    .with_title("PRISM Tool Schema")
                    .no_annotation(),
                RawResourceTemplate::new(SEARCH_RESOURCE_TEMPLATE_URI, "PRISM Search")
                    .with_description(
                        "Read structured PRISM search results and diagnostics for a query string with optional `limit`, `cursor`, `strategy`, `ownerKind`, `kind`, `path`, `module`, `taskId`, and `includeInferred` options",
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
        })
    }
}

impl PrismMcpServer {
    fn server_instructions(&self) -> String {
        let mut instructions = String::from(
            "Start with prism://capabilities for the canonical map of tools, query methods, resources, feature gates, and build info, then use prism://api-reference for the typed query contract and prism://schemas for the JSON Schema catalog. Use prism://tool-schemas and prism://schema/tool/{toolName} when you need exact MCP tool input shapes. Prefer the compact staged path for default agent work: prism_locate to pick a target, prism_open to inspect a bounded slice, prism_workset for compact surrounding context, prism_expand for explicit depth, and prism_query only when the compact path cannot express the needed read. Use prism://session to inspect the active workspace, task, runtime limits, and active feature flags, prism://plans for browseable plan discovery, prism_session to change task context or limits, prism://entrypoints for a quick workspace overview, prism://search/{query} for browseable search results, prism://symbol/{crateName}/{kind}/{path} for exact symbol snapshots, prism://lineage/{lineageId} for symbol history, prism://task/{taskId} for recorded task outcomes, and prism://event/{eventId}, prism://memory/{memoryId}, and prism://edge/{edgeId} for mutation outputs. Follow each resource payload's schemaUri and relatedResources fields instead of reconstructing URIs by convention. Use prism_mutate for outcomes, anchored memory, validation feedback, inferred edges, coordination state, claims, artifacts, and curator proposal decisions.",
        );

        if self.host.features.mode_label() != "full" {
            instructions.push_str(" Coordination features are gated on this server; check prism://session before using plan, claim, or artifact workflows.");
        }

        instructions
    }
}
