use prism_js::SymbolView;
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    service::RequestContext,
    tool, tool_handler, tool_router, ErrorData as McpError, RoleServer, ServerHandler,
};
use serde_json::json;

use crate::*;

impl PrismMcpServer {
    pub(crate) fn build_tool_router() -> ToolRouter<Self> {
        Self::tool_router()
    }
}

#[tool_router]
impl PrismMcpServer {
    #[tool(
        description = "Create and activate a task context for subsequent mutations in this session.",
        annotations(
            title = "Start PRISM Task",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        ),
        output_schema = rmcp::handler::server::tool::schema_for_output::<PrismStartTaskResult>()
            .unwrap()
    )]
    fn prism_start_task(
        &self,
        Parameters(args): Parameters<PrismStartTaskArgs>,
    ) -> Result<CallToolResult, McpError> {
        if args.description.trim().is_empty() {
            return Err(McpError::invalid_params(
                "task description cannot be empty",
                Some(json!({ "field": "description" })),
            ));
        }

        let task = self
            .host
            .start_task(args.description, args.tags.unwrap_or_default())
            .map_err(map_query_error)?;
        let task_id = task.0.to_string();
        structured_tool_result_with_links(
            PrismStartTaskResult {
                task_id: task_id.clone(),
            },
            vec![session_resource_link(), task_resource_link(&task_id)],
        )
    }

    #[tool(
        description = "Inspect the current MCP session state, including workspace root, active task, and runtime limits.",
        annotations(title = "Get PRISM Session", read_only_hint = true),
        output_schema = rmcp::handler::server::tool::schema_for_output::<SessionView>().unwrap()
    )]
    fn prism_get_session(
        &self,
        Parameters(_args): Parameters<PrismGetSessionArgs>,
    ) -> Result<CallToolResult, McpError> {
        let session = self.host.session_view().map_err(map_query_error)?;
        let mut links = vec![session_resource_link()];
        if let Some(task) = &session.current_task {
            links.push(task_resource_link(&task.task_id));
        }
        structured_tool_result_with_links(session, links)
    }

    #[tool(
        description = "Configure session-scoped limits and the active task context for subsequent mutations.",
        annotations(
            title = "Configure PRISM Session",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        ),
        output_schema = rmcp::handler::server::tool::schema_for_output::<SessionView>().unwrap()
    )]
    fn prism_configure_session(
        &self,
        Parameters(args): Parameters<PrismConfigureSessionArgs>,
    ) -> Result<CallToolResult, McpError> {
        let session = self.host.configure_session(args).map_err(map_query_error)?;
        let mut links = vec![session_resource_link()];
        if let Some(task) = &session.current_task {
            links.push(task_resource_link(&task.task_id));
        }
        structured_tool_result_with_links(session, links)
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
            .execute(&args.code, language)
            .map_err(map_query_error)?;
        structured_tool_result(envelope)
    }

    #[tool(
        description = "Convenience lookup for the best matching symbol. Returns the same structured query envelope as prism_query.",
        annotations(title = "Lookup PRISM Symbol", read_only_hint = true),
        output_schema = rmcp::handler::server::tool::schema_for_output::<QueryEnvelopeSchema>()
            .unwrap()
    )]
    fn prism_symbol(
        &self,
        Parameters(args): Parameters<PrismSymbolArgs>,
    ) -> Result<CallToolResult, McpError> {
        if args.query.trim().is_empty() {
            return Err(McpError::invalid_params(
                "query cannot be empty",
                Some(json!({ "field": "query" })),
            ));
        }

        let envelope = self
            .host
            .symbol_query(&args.query)
            .map_err(map_query_error)?;
        let links = serde_json::from_value::<Option<SymbolView>>(envelope.result.clone())
            .ok()
            .flatten()
            .map(|symbol| symbol_links(&symbol))
            .unwrap_or_default();
        structured_tool_result_with_links(envelope, links)
    }

    #[tool(
        description = "Convenience search lookup. Returns the same structured query envelope as prism_query.",
        annotations(title = "Search PRISM Graph", read_only_hint = true),
        output_schema = rmcp::handler::server::tool::schema_for_output::<QueryEnvelopeSchema>()
            .unwrap()
    )]
    fn prism_search(
        &self,
        Parameters(args): Parameters<PrismSearchArgs>,
    ) -> Result<CallToolResult, McpError> {
        if args.query.trim().is_empty() {
            return Err(McpError::invalid_params(
                "query cannot be empty",
                Some(json!({ "field": "query" })),
            ));
        }

        let query = args.query.clone();
        let envelope = self
            .host
            .search_query(SearchArgs {
                query: query.clone(),
                limit: args.limit,
                kind: args.kind,
                path: args.path,
                include_inferred: None,
            })
            .map_err(map_query_error)?;
        let mut links = vec![search_resource_link(&query)];
        if let Ok(symbols) = serde_json::from_value::<Vec<SymbolView>>(envelope.result.clone()) {
            for symbol in symbols.iter().take(8) {
                links.push(symbol_resource_link(symbol));
            }
        }
        structured_tool_result_with_links(envelope, links)
    }

    #[tool(
        description = "Write a structured outcome event for the current task or symbol anchors.",
        annotations(
            title = "Record Outcome Event",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        ),
        output_schema = rmcp::handler::server::tool::schema_for_output::<EventMutationResult>()
            .unwrap()
    )]
    fn prism_outcome(
        &self,
        Parameters(args): Parameters<PrismOutcomeArgs>,
    ) -> Result<CallToolResult, McpError> {
        let result = self.host.store_outcome(args).map_err(map_query_error)?;
        structured_tool_result_with_links(
            result.clone(),
            vec![
                event_resource_link(&result.event_id),
                task_resource_link(&result.task_id),
            ],
        )
    }

    #[tool(
        description = "Store an agent note anchored to nodes or lineages.",
        annotations(
            title = "Store Agent Note",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        ),
        output_schema = rmcp::handler::server::tool::schema_for_output::<MemoryMutationResult>()
            .unwrap()
    )]
    fn prism_note(
        &self,
        Parameters(args): Parameters<PrismNoteArgs>,
    ) -> Result<CallToolResult, McpError> {
        let result = self.host.store_note(args).map_err(map_query_error)?;
        structured_tool_result_with_links(
            result.clone(),
            vec![
                memory_resource_link(&result.memory_id),
                task_resource_link(&result.task_id),
            ],
        )
    }

    #[tool(
        description = "Persist an inferred edge into the session overlay or a promoted scope.",
        annotations(
            title = "Store Inferred Edge",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        ),
        output_schema = rmcp::handler::server::tool::schema_for_output::<EdgeMutationResult>()
            .unwrap()
    )]
    fn prism_infer_edge(
        &self,
        Parameters(args): Parameters<PrismInferEdgeArgs>,
    ) -> Result<CallToolResult, McpError> {
        let result = self
            .host
            .store_inferred_edge(args)
            .map_err(map_query_error)?;
        structured_tool_result_with_links(
            result.clone(),
            vec![
                edge_resource_link(&result.edge_id),
                task_resource_link(&result.task_id),
            ],
        )
    }

    #[tool(
        description = "Mutate shared coordination state for plans, tasks, and handoffs.",
        annotations(
            title = "Mutate Coordination State",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        ),
        output_schema = rmcp::handler::server::tool::schema_for_output::<CoordinationMutationResult>()
            .unwrap()
    )]
    fn prism_coordination(
        &self,
        Parameters(args): Parameters<PrismCoordinationArgs>,
    ) -> Result<CallToolResult, McpError> {
        let result = self
            .host
            .store_coordination(args)
            .map_err(map_query_error)?;
        structured_tool_result(result)
    }

    #[tool(
        description = "Acquire, renew, or release shared work claims.",
        annotations(
            title = "Mutate Claims",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        ),
        output_schema = rmcp::handler::server::tool::schema_for_output::<ClaimMutationResult>()
            .unwrap()
    )]
    fn prism_claim(
        &self,
        Parameters(args): Parameters<PrismClaimArgs>,
    ) -> Result<CallToolResult, McpError> {
        let result = self.host.store_claim(args).map_err(map_query_error)?;
        structured_tool_result(result)
    }

    #[tool(
        description = "Propose, supersede, or review shared artifacts.",
        annotations(
            title = "Mutate Artifacts",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        ),
        output_schema = rmcp::handler::server::tool::schema_for_output::<ArtifactMutationResult>()
            .unwrap()
    )]
    fn prism_artifact(
        &self,
        Parameters(args): Parameters<PrismArtifactArgs>,
    ) -> Result<CallToolResult, McpError> {
        let result = self.host.store_artifact(args).map_err(map_query_error)?;
        structured_tool_result(result)
    }

    #[tool(
        description = "Convenience outcome for a test run.",
        annotations(
            title = "Record Test Run",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        ),
        output_schema = rmcp::handler::server::tool::schema_for_output::<EventMutationResult>()
            .unwrap()
    )]
    fn prism_test_ran(
        &self,
        Parameters(args): Parameters<PrismTestRanArgs>,
    ) -> Result<CallToolResult, McpError> {
        let summary = format!(
            "test `{}` {}",
            args.test,
            if args.passed { "passed" } else { "failed" }
        );
        let result = self
            .host
            .store_outcome(PrismOutcomeArgs {
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
            })
            .map_err(map_query_error)?;
        structured_tool_result_with_links(
            result.clone(),
            vec![
                event_resource_link(&result.event_id),
                task_resource_link(&result.task_id),
            ],
        )
    }

    #[tool(
        description = "Convenience outcome for an observed failure.",
        annotations(
            title = "Record Observed Failure",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        ),
        output_schema = rmcp::handler::server::tool::schema_for_output::<EventMutationResult>()
            .unwrap()
    )]
    fn prism_failure_observed(
        &self,
        Parameters(args): Parameters<PrismFailureObservedArgs>,
    ) -> Result<CallToolResult, McpError> {
        let evidence = args
            .trace
            .map(|trace| vec![OutcomeEvidenceInput::StackTrace { hash: trace }]);
        let result = self
            .host
            .store_outcome(PrismOutcomeArgs {
                kind: OutcomeKindInput::FailureObserved,
                anchors: args.anchors,
                summary: args.summary,
                result: Some(OutcomeResultInput::Failure),
                evidence,
                task_id: args.task_id,
            })
            .map_err(map_query_error)?;
        structured_tool_result_with_links(
            result.clone(),
            vec![
                event_resource_link(&result.event_id),
                task_resource_link(&result.task_id),
            ],
        )
    }

    #[tool(
        description = "Convenience outcome for a validated fix.",
        annotations(
            title = "Record Validated Fix",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        ),
        output_schema = rmcp::handler::server::tool::schema_for_output::<EventMutationResult>()
            .unwrap()
    )]
    fn prism_fix_validated(
        &self,
        Parameters(args): Parameters<PrismFixValidatedArgs>,
    ) -> Result<CallToolResult, McpError> {
        let result = self
            .host
            .store_outcome(PrismOutcomeArgs {
                kind: OutcomeKindInput::FixValidated,
                anchors: args.anchors,
                summary: args.summary,
                result: Some(OutcomeResultInput::Success),
                evidence: None,
                task_id: args.task_id,
            })
            .map_err(map_query_error)?;
        structured_tool_result_with_links(
            result.clone(),
            vec![
                event_resource_link(&result.event_id),
                task_resource_link(&result.task_id),
            ],
        )
    }

    #[tool(
        description = "Promote a completed curator inferred-edge proposal into the session overlay or persisted inference store.",
        annotations(
            title = "Promote Curator Edge",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        ),
        output_schema = rmcp::handler::server::tool::schema_for_output::<CuratorProposalDecisionResult>()
            .unwrap()
    )]
    fn prism_curator_promote_edge(
        &self,
        Parameters(args): Parameters<PrismCuratorPromoteEdgeArgs>,
    ) -> Result<CallToolResult, McpError> {
        let result = self
            .host
            .promote_curator_edge(args)
            .map_err(map_query_error)?;
        let mut links = vec![session_resource_link()];
        if let Some(memory_id) = &result.memory_id {
            links.push(memory_resource_link(memory_id));
        }
        if let Some(edge_id) = &result.edge_id {
            links.push(edge_resource_link(edge_id));
        }
        structured_tool_result_with_links(result, links)
    }

    #[tool(
        description = "Promote a completed curator structural-memory, risk-summary, or validation-recipe proposal into durable PRISM memory.",
        annotations(
            title = "Promote Curator Memory",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        ),
        output_schema = rmcp::handler::server::tool::schema_for_output::<CuratorProposalDecisionResult>()
            .unwrap()
    )]
    fn prism_curator_promote_memory(
        &self,
        Parameters(args): Parameters<PrismCuratorPromoteMemoryArgs>,
    ) -> Result<CallToolResult, McpError> {
        let result = self
            .host
            .promote_curator_memory(args)
            .map_err(map_query_error)?;
        let mut links = vec![session_resource_link()];
        if let Some(memory_id) = &result.memory_id {
            links.push(memory_resource_link(memory_id));
        }
        if let Some(edge_id) = &result.edge_id {
            links.push(edge_resource_link(edge_id));
        }
        structured_tool_result_with_links(result, links)
    }

    #[tool(
        description = "Reject a curator proposal without mutating the graph. Use this for risk summaries, validation recipes, or inferred edges you do not want to apply.",
        annotations(
            title = "Reject Curator Proposal",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        ),
        output_schema = rmcp::handler::server::tool::schema_for_output::<CuratorProposalDecisionResult>()
            .unwrap()
    )]
    fn prism_curator_reject_proposal(
        &self,
        Parameters(args): Parameters<PrismCuratorRejectProposalArgs>,
    ) -> Result<CallToolResult, McpError> {
        let result = self
            .host
            .reject_curator_proposal(args)
            .map_err(map_query_error)?;
        structured_tool_result_with_links(result, vec![session_resource_link()])
    }
}

#[tool_handler]
impl ServerHandler for PrismMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_resources()
                .enable_tools()
                .build(),
        )
        .with_server_info(Implementation::from_build_env())
        .with_instructions(
            "Start with prism://api-reference for the typed query contract and prism://schemas for the JSON Schema catalog. Use prism_get_session or prism://session to inspect the active workspace, task, and runtime limits, prism_configure_session to change them, prism_query for programmable read-only graph queries, prism_symbol or prism_search for direct lookups, prism://entrypoints for a quick workspace overview, prism://search/{query} for browseable search results, prism://symbol/{crateName}/{kind}/{path} for exact symbol snapshots, prism://lineage/{lineageId} for symbol history, prism://task/{taskId} for recorded task outcomes, prism://event/{eventId}, prism://memory/{memoryId}, and prism://edge/{edgeId} for mutation outputs. Follow each resource payload's schemaUri and relatedResources fields instead of reconstructing URIs by convention. Use the prism_* mutation tools to record outcomes, notes, inferred edges, task context, and curator proposal decisions.",
        )
        .with_protocol_version(ProtocolVersion::LATEST)
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
            ResourceContents::text(api_reference_markdown(), request.uri.clone())
                .with_mime_type("text/markdown")
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
        } else if base_uri == SESSION_URI {
            json_resource_contents_with_meta(
                self.host
                    .session_resource_value()
                    .map_err(map_query_error)?,
                request.uri.clone(),
                Some(resource_meta(
                    "session",
                    Some(schema_resource_uri("session")),
                    None,
                )),
            )?
        } else if base_uri == ENTRYPOINTS_URI {
            json_resource_contents_with_meta(
                self.host
                    .entrypoints_resource_value(uri)
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
                    .search_resource_value(uri, &query)
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
                    .symbol_resource_value(&id)
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
                    .lineage_resource_value(uri, &lineage)
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
                    .task_resource_value(uri, &task_id)
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
                    .memory_resource_value(&memory_id)
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
                    .edge_resource_value(&edge_id)
                    .map_err(map_query_error)?,
                request.uri.clone(),
                Some(resource_meta(
                    "edge",
                    Some(schema_resource_uri("edge")),
                    None,
                )),
            )?
        } else if let Some(resource_kind) = parse_schema_resource_uri(uri) {
            match resource_kind.as_str() {
                "session" => schema_resource_contents::<SessionResourcePayload>(
                    uri,
                    "PRISM Session Resource Schema",
                    "JSON Schema for the PRISM session resource payload.",
                    "session",
                )?,
                "schemas" => schema_resource_contents::<ResourceSchemaCatalogPayload>(
                    uri,
                    "PRISM Resource Schema Catalog Schema",
                    "JSON Schema for the PRISM resource schema catalog payload.",
                    "schemas",
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
                RawResourceTemplate::new(SCHEMA_RESOURCE_TEMPLATE_URI, "PRISM Resource Schema")
                    .with_description(
                        "Read a JSON Schema document for a structured PRISM resource payload kind such as `session`, `entrypoints`, `search`, `symbol`, `lineage`, `task`, `event`, `memory`, or `edge`",
                    )
                    .with_mime_type("application/schema+json")
                    .with_title("PRISM Resource Schema")
                    .no_annotation(),
                RawResourceTemplate::new(SEARCH_RESOURCE_TEMPLATE_URI, "PRISM Search")
                    .with_description(
                        "Read structured PRISM search results and diagnostics for a query string with optional `limit` and opaque `cursor` pagination",
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
