use anyhow::{anyhow, Result};
use prism_agent::EdgeId;
use prism_ir::{AnchorRef, CoordinationTaskId, EventId, LineageId, NodeId, PlanId, TaskId};
use prism_memory::{MemoryEventQuery, MemoryId};
use serde_json::json;
use std::sync::Arc;
use std::time::Instant;

use crate::file_queries::file_read;
use crate::{
    anchor_resource_view_links, capabilities_resource_uri, capabilities_resource_value,
    capabilities_resource_view_link, co_change_view, compact_discovery_bundle_candidate_excerpts,
    compact_owner_candidate_excerpts, contract_packet_view,
    contracts_resource_view_link_with_options, dedupe_resource_link_views, derive_task_metadata,
    discovery_bundle_view, edge_resource_uri, edge_resource_view_link, event_resource_view_link,
    file_resource_view_link, inferred_edge_record_view, instructions_resource_view_link,
    lineage_event_view, lineage_resource_view_link, lineage_status, memory_entry_view,
    memory_event_view, memory_resource_uri, memory_resource_view_link, owner_views_for_query,
    paginate_items, parse_resource_page, parse_resource_query_param, plan_resource_uri,
    plan_resource_view_link, plan_summary_view, plan_view, plans_resource_view_link,
    plans_resource_view_link_with_options, resource_link_view, resource_schema_catalog_entries,
    schema_resource_uri, schema_resource_view_link, schemas_resource_uri,
    schemas_resource_view_link, search_ambiguity_from_diagnostics,
    search_resource_view_link_with_options, session_resource_uri, session_resource_view_link,
    symbol_for, symbol_resource_uri, symbol_resource_view_link, symbol_resource_view_link_for_id,
    symbol_view, symbol_views_for_ids, task_heartbeat_advice, task_heartbeat_next_action,
    task_resource_view_link, task_resource_view_links_from_events, tool_schemas_resource_value,
    tool_schemas_resource_view_link, vocab_resource_value, vocab_resource_view_link,
    workspace_revision_view, CapabilitiesResourcePayload, ContractsResourcePayload,
    CoordinationFeaturesView, EdgeResourcePayload, EntrypointsResourcePayload,
    EventResourcePayload, FeatureFlagsView, FileResourcePayload, InferredEdgeRecordView,
    LineageResourcePayload, MemoryResourcePayload, PlanResourcePayload, PlansQueryArgs,
    PlansResourcePayload, QueryExecution, QueryHost, ResourceSchemaCatalogPayload, SearchArgs,
    SearchResourcePayload, SessionLimitsView, SessionRepairActionView, SessionResourcePayload,
    SessionState, SessionTaskView, SessionView, SymbolResourcePayload, TaskHeartbeatAdvice,
    TaskResourcePayload, VocabularyResourcePayload, DEFAULT_RESOURCE_PAGE_LIMIT,
    DEFAULT_TASK_JOURNAL_EVENT_LIMIT, DEFAULT_TASK_JOURNAL_MEMORY_LIMIT, ENTRYPOINTS_URI,
};

impl QueryHost {
    fn execute_traced_resource_read<T, F>(
        &self,
        resource_kind: &str,
        target: &str,
        operation: F,
    ) -> Result<T>
    where
        F: FnOnce() -> Result<T>,
    {
        let refresh_started = Instant::now();
        let refresh = match self.observe_workspace_for_read() {
            Ok(refresh) => {
                crate::refresh_phases::record_resource_runtime_sync_phases(&refresh);
                crate::resource_trace::record_phase(
                    "resource.refreshWorkspace",
                    &json!({
                        "resourceKind": resource_kind,
                        "target": target,
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
                );
                refresh
            }
            Err(error) => {
                crate::resource_trace::record_phase(
                    "resource.refreshWorkspace",
                    &json!({
                        "resourceKind": resource_kind,
                        "target": target,
                        "refreshPath": "error",
                    }),
                    refresh_started.elapsed(),
                    false,
                    Some(error.to_string()),
                );
                return Err(error);
            }
        };
        let handler_started = Instant::now();
        match operation() {
            Ok(result) => {
                crate::resource_trace::record_phase(
                    "resource.handler",
                    &json!({
                        "resourceKind": resource_kind,
                        "target": target,
                        "refreshPath": refresh.refresh_path,
                    }),
                    handler_started.elapsed(),
                    true,
                    None,
                );
                Ok(result)
            }
            Err(error) => {
                crate::resource_trace::record_phase(
                    "resource.handler",
                    &json!({
                        "resourceKind": resource_kind,
                        "target": target,
                        "refreshPath": refresh.refresh_path,
                    }),
                    handler_started.elapsed(),
                    false,
                    Some(error.to_string()),
                );
                Err(error)
            }
        }
    }

    pub(crate) fn session_view_without_refresh(&self, session: &SessionState) -> SessionView {
        let limits = session.limits();
        let current_task = session
            .current_task_state()
            .map(|task| session_task_view(self, session, &task));
        SessionView {
            workspace_root: self.workspace_root().map(|root| root.display().to_string()),
            current_task,
            current_agent: session.current_agent().map(|agent| agent.0.to_string()),
            limits: SessionLimitsView {
                max_result_nodes: limits.max_result_nodes,
                max_call_graph_depth: limits.max_call_graph_depth,
                max_output_json_bytes: limits.max_output_json_bytes,
            },
            features: FeatureFlagsView {
                mode: self.features.mode_label().to_string(),
                coordination: CoordinationFeaturesView {
                    workflow: self.features.coordination.workflow,
                    claims: self.features.coordination.claims,
                    artifacts: self.features.coordination.artifacts,
                },
                internal_developer: self.features.internal_developer,
            },
        }
    }

    pub(crate) fn session_resource_value(
        &self,
        session: &SessionState,
    ) -> Result<SessionResourcePayload> {
        let uri = session_resource_uri();
        self.execute_traced_resource_read("session", &uri, || {
            let schema_uri = schema_resource_uri("session");
            let session = self.session_view_without_refresh(session);
            let mut related_resources = vec![
                instructions_resource_view_link(),
                capabilities_resource_view_link(),
                session_resource_view_link(),
                vocab_resource_view_link(),
                schema_resource_view_link("session"),
                schemas_resource_view_link(),
                resource_link_view(
                    ENTRYPOINTS_URI.to_string(),
                    "PRISM Entrypoints",
                    "Workspace entrypoints and top-level starting symbols",
                ),
            ];
            if let Some(task) = &session.current_task {
                related_resources.push(task_resource_view_link(&task.task_id));
            }
            Ok(SessionResourcePayload {
                uri: uri.clone(),
                schema_uri,
                workspace_root: session.workspace_root,
                current_task: session.current_task,
                current_agent: session.current_agent,
                limits: session.limits,
                features: session.features,
                related_resources: dedupe_resource_link_views(related_resources),
            })
        })
    }

    pub(crate) fn capabilities_resource_value(&self) -> Result<CapabilitiesResourcePayload> {
        let uri = capabilities_resource_uri();
        self.execute_traced_resource_read("capabilities", &uri, || {
            capabilities_resource_value(self)
        })
    }

    pub(crate) fn schemas_resource_value(&self) -> ResourceSchemaCatalogPayload {
        let mut related_resources = vec![
            capabilities_resource_view_link(),
            schemas_resource_view_link(),
            vocab_resource_view_link(),
            schema_resource_view_link("schemas"),
            tool_schemas_resource_view_link(),
            session_resource_view_link(),
            resource_link_view(
                ENTRYPOINTS_URI.to_string(),
                "PRISM Entrypoints",
                "Workspace entrypoints and top-level starting symbols",
            ),
        ];
        related_resources.extend(
            resource_schema_catalog_entries()
                .iter()
                .map(|entry| schema_resource_view_link(&entry.resource_kind)),
        );
        ResourceSchemaCatalogPayload {
            uri: schemas_resource_uri(),
            schema_uri: schema_resource_uri("schemas"),
            schemas: resource_schema_catalog_entries(),
            related_resources: dedupe_resource_link_views(related_resources),
        }
    }

    pub(crate) fn tool_schemas_resource_value(&self) -> crate::ToolSchemaCatalogPayload {
        tool_schemas_resource_value()
    }

    pub(crate) fn vocab_resource_value(&self) -> VocabularyResourcePayload {
        vocab_resource_value()
    }

    pub(crate) fn task_metadata(
        &self,
        session: &SessionState,
        task_id: &TaskId,
    ) -> crate::ResolvedTaskMetadata {
        let prism = self.current_prism();
        let replay = crate::load_task_replay(self.workspace_session_ref(), prism.as_ref(), task_id)
            .unwrap_or_else(|_| prism.resume_task(task_id));
        derive_task_metadata(
            session.current_task_state().as_ref(),
            prism.as_ref(),
            task_id,
            &replay.events,
            None,
        )
    }

    pub(crate) fn entrypoints_resource_value(
        &self,
        session: Arc<SessionState>,
        uri: &str,
    ) -> Result<EntrypointsResourcePayload> {
        self.execute_traced_resource_read("entrypoints", uri, || {
            let schema_uri = schema_resource_uri("entrypoints");
            let prism = self.current_prism();
            let execution = QueryExecution::new(
                self.clone(),
                Arc::clone(&session),
                prism,
                self.begin_query_run(
                    session.as_ref(),
                    "read_resource",
                    "resource",
                    "prism://entrypoints",
                ),
            );
            let paged = paginate_items(
                execution.entrypoints()?,
                parse_resource_page(
                    uri,
                    DEFAULT_RESOURCE_PAGE_LIMIT,
                    session.limits().max_result_nodes,
                )?,
            );
            let mut related_resources = vec![
                session_resource_view_link(),
                schema_resource_view_link("entrypoints"),
                schemas_resource_view_link(),
                resource_link_view(
                    uri.to_string(),
                    "PRISM Entrypoints",
                    "Workspace entrypoints and top-level starting symbols",
                ),
            ];
            related_resources.extend(paged.items.iter().take(8).map(symbol_resource_view_link));
            Ok(EntrypointsResourcePayload {
                uri: uri.to_string(),
                schema_uri,
                entrypoints: paged.items,
                page: paged.page,
                truncated: paged.truncated,
                diagnostics: execution.diagnostics(),
                related_resources: dedupe_resource_link_views(related_resources),
            })
        })
    }

    pub(crate) fn file_resource_value(
        &self,
        session: &SessionState,
        uri: &str,
        args: crate::FileReadArgs,
    ) -> Result<FileResourcePayload> {
        self.execute_traced_resource_read("file", uri, || {
            let schema_uri = schema_resource_uri("file");
            let prism = self.current_prism();
            let excerpt = file_read(self, args.clone())?;
            let related_resources = dedupe_resource_link_views(vec![
                session_resource_view_link(),
                schema_resource_view_link("file"),
                schemas_resource_view_link(),
                file_resource_view_link(&args.path),
            ]);
            let _ = session;
            Ok(FileResourcePayload {
                uri: uri.to_string(),
                schema_uri,
                workspace_revision: workspace_revision_view(prism.workspace_revision()),
                path: args.path.clone(),
                excerpt,
                related_resources,
            })
        })
    }

    pub(crate) fn plans_resource_value(
        &self,
        session: Arc<SessionState>,
        uri: &str,
    ) -> Result<PlansResourcePayload> {
        self.execute_traced_resource_read("plans", uri, || {
            let schema_uri = schema_resource_uri("plans");
            let prism = self.current_prism();
            let execution = QueryExecution::new(
                self.clone(),
                Arc::clone(&session),
                prism.clone(),
                self.begin_query_run(
                    session.as_ref(),
                    "read_resource",
                    "resource",
                    "prism://plans",
                ),
            );
            let status =
                parse_resource_query_param(uri, "status").filter(|value| !value.is_empty());
            let scope = parse_resource_query_param(uri, "scope").filter(|value| !value.is_empty());
            let contains =
                parse_resource_query_param(uri, "contains").filter(|value| !value.is_empty());
            let paged = paginate_items(
                execution.plans(PlansQueryArgs {
                    status: status.clone(),
                    scope: scope.clone(),
                    contains: contains.clone(),
                    limit: Some(session.limits().max_result_nodes),
                })?,
                parse_resource_page(
                    uri,
                    DEFAULT_RESOURCE_PAGE_LIMIT,
                    session.limits().max_result_nodes,
                )?,
            );
            let related_resources = vec![
                session_resource_view_link(),
                schema_resource_view_link("plans"),
                schemas_resource_view_link(),
                if status.is_none() && scope.is_none() && contains.is_none() {
                    plans_resource_view_link()
                } else {
                    plans_resource_view_link_with_options(
                        status.as_deref(),
                        scope.as_deref(),
                        contains.as_deref(),
                    )
                },
            ];
            let mut related_resources = related_resources;
            related_resources.extend(
                paged
                    .items
                    .iter()
                    .map(|plan| plan_resource_view_link(&plan.plan_id)),
            );
            Ok(PlansResourcePayload {
                uri: uri.to_string(),
                schema_uri,
                workspace_revision: workspace_revision_view(prism.workspace_revision()),
                status,
                scope,
                contains,
                plans: paged.items,
                page: paged.page,
                truncated: paged.truncated,
                diagnostics: execution.diagnostics(),
                related_resources: dedupe_resource_link_views(related_resources),
            })
        })
    }

    pub(crate) fn plan_resource_value(&self, plan_id: &PlanId) -> Result<PlanResourcePayload> {
        let uri = plan_resource_uri(&plan_id.0);
        self.execute_traced_resource_read("plan", &uri, || {
            let schema_uri = schema_resource_uri("plan");
            let prism = self.current_prism();
            let plan = prism
                .coordination_plan(plan_id)
                .ok_or_else(|| anyhow!("unknown plan `{}`", plan_id.0))?;
            let root_node_ids = prism
                .plan_graph(plan_id)
                .map(|graph| graph.root_nodes)
                .unwrap_or_else(|| {
                    plan.root_tasks
                        .iter()
                        .map(|task_id| prism_ir::PlanNodeId::new(task_id.0.clone()))
                        .collect()
                });
            let plan = plan_view(plan, root_node_ids);
            let summary = prism.plan_summary(plan_id).map(plan_summary_view);
            let related_resources = vec![
                session_resource_view_link(),
                plans_resource_view_link(),
                plan_resource_view_link(&plan.id),
                schema_resource_view_link("plan"),
                schemas_resource_view_link(),
            ];
            Ok(PlanResourcePayload {
                uri: uri.clone(),
                schema_uri,
                workspace_revision: workspace_revision_view(prism.workspace_revision()),
                plan,
                summary,
                related_resources: dedupe_resource_link_views(related_resources),
            })
        })
    }

    pub(crate) fn contracts_resource_value(
        &self,
        session: Arc<SessionState>,
        uri: &str,
    ) -> Result<ContractsResourcePayload> {
        self.execute_traced_resource_read("contracts", uri, || {
            let schema_uri = schema_resource_uri("contracts");
            let prism = self.current_prism();
            let execution = QueryExecution::new(
                self.clone(),
                Arc::clone(&session),
                prism.clone(),
                self.begin_query_run(
                    session.as_ref(),
                    "read_resource",
                    "resource",
                    "prism://contracts",
                ),
            );
            let contains =
                parse_resource_query_param(uri, "contains").filter(|value| !value.is_empty());
            let status =
                parse_resource_query_param(uri, "status").filter(|value| !value.is_empty());
            let scope = parse_resource_query_param(uri, "scope").filter(|value| !value.is_empty());
            let kind = parse_resource_query_param(uri, "kind").filter(|value| !value.is_empty());

            let contracts = if let Some(query) = contains.as_deref() {
                prism
                    .resolve_contracts(query, session.limits().max_result_nodes)
                    .into_iter()
                    .map(|resolution| {
                        let packet = resolution.packet.clone();
                        contract_packet_view(
                            prism.as_ref(),
                            self.workspace_root(),
                            packet,
                            Some(resolution),
                        )
                    })
                    .collect::<Vec<_>>()
            } else {
                prism
                    .curated_contracts()
                    .into_iter()
                    .map(|packet| {
                        contract_packet_view(prism.as_ref(), self.workspace_root(), packet, None)
                    })
                    .collect::<Vec<_>>()
            };
            let contracts = contracts
                .into_iter()
                .filter(|contract| {
                    status
                        .as_deref()
                        .is_none_or(|value| contract_status_label(&contract.status) == value)
                })
                .filter(|contract| {
                    scope
                        .as_deref()
                        .is_none_or(|value| contract_scope_label(&contract.scope) == value)
                })
                .filter(|contract| {
                    kind.as_deref()
                        .is_none_or(|value| contract_kind_label(&contract.kind) == value)
                })
                .collect::<Vec<_>>();
            let paged = paginate_items(
                contracts,
                parse_resource_page(
                    uri,
                    DEFAULT_RESOURCE_PAGE_LIMIT,
                    session.limits().max_result_nodes,
                )?,
            );
            let mut related_resources = vec![
                session_resource_view_link(),
                schema_resource_view_link("contracts"),
                schemas_resource_view_link(),
                contracts_resource_view_link_with_options(
                    contains.as_deref(),
                    status.as_deref(),
                    scope.as_deref(),
                    kind.as_deref(),
                ),
            ];
            related_resources.extend(paged.items.iter().flat_map(|contract| {
                contract
                    .subject
                    .anchors
                    .iter()
                    .filter_map(|anchor| match anchor {
                        crate::AnchorRefView::Node {
                            crate_name,
                            path,
                            kind,
                        } => crate::parse_node_kind(kind).ok().map(|kind| {
                            symbol_resource_view_link_for_id(&NodeId::new(
                                crate_name.clone(),
                                path.clone(),
                                kind,
                            ))
                        }),
                        crate::AnchorRefView::Lineage { lineage_id } => {
                            Some(lineage_resource_view_link(lineage_id))
                        }
                        crate::AnchorRefView::File { path, .. } => {
                            path.as_deref().map(file_resource_view_link)
                        }
                        crate::AnchorRefView::Kind { .. } => None,
                    })
            }));
            Ok(ContractsResourcePayload {
                uri: uri.to_string(),
                schema_uri,
                workspace_revision: workspace_revision_view(prism.workspace_revision()),
                contains,
                status,
                scope,
                kind,
                contracts: paged.items,
                page: paged.page,
                truncated: paged.truncated,
                diagnostics: execution.diagnostics(),
                related_resources: dedupe_resource_link_views(related_resources),
            })
        })
    }

    pub(crate) fn symbol_resource_value(
        &self,
        session: Arc<SessionState>,
        id: &NodeId,
    ) -> Result<SymbolResourcePayload> {
        let target = format!("prism://symbol/{}", id.path);
        self.execute_traced_resource_read("symbol", &target, || {
            let schema_uri = schema_resource_uri("symbol");
            let prism = self.current_prism();
            let execution = QueryExecution::new(
                self.clone(),
                Arc::clone(&session),
                prism.clone(),
                self.begin_query_run(
                    session.as_ref(),
                    "read_resource",
                    "resource",
                    target.clone(),
                ),
            );
            let symbol = symbol_for(prism.as_ref(), id)?;
            let symbol = symbol_view(prism.as_ref(), &symbol)?;
            let mut discovery = discovery_bundle_view(prism.as_ref(), session.as_ref(), id)?;
            compact_discovery_bundle_candidate_excerpts(prism.as_ref(), &mut discovery)?;
            let suggested_reads = discovery.suggested_reads.clone();
            let read_context = discovery.read_context.clone();
            let edit_context = discovery.edit_context.clone();
            let suggested_queries = discovery.suggested_queries.clone();
            let relations = discovery.relations.clone();
            let spec_cluster = discovery.spec_cluster.clone();
            let spec_drift = discovery.spec_drift.clone();
            let lineage = discovery.lineage.clone();
            let co_change_neighbors = discovery.co_change_neighbors.clone();
            let related_failures = discovery.related_failures.clone();
            let blast_radius = discovery.blast_radius.clone();
            let validation_recipe = discovery.validation_recipe.clone();
            let mut related_resources = vec![symbol_resource_view_link(&symbol)];
            related_resources.extend(
                suggested_reads
                    .iter()
                    .map(|candidate| symbol_resource_view_link(&candidate.symbol)),
            );
            if let Some(lineage) = &lineage {
                related_resources.push(lineage_resource_view_link(&lineage.lineage_id));
            }
            related_resources.extend(task_resource_view_links_from_events(
                &prism
                    .outcome_memory()
                    .outcomes_for(&[AnchorRef::Node(id.clone())], 16),
            ));
            related_resources.extend(
                related_failures
                    .iter()
                    .map(|event| event_resource_view_link(event.meta.id.0.as_str())),
            );
            related_resources.push(session_resource_view_link());
            related_resources.push(schema_resource_view_link("symbol"));
            related_resources.push(schemas_resource_view_link());
            Ok(SymbolResourcePayload {
                uri: symbol_resource_uri(&symbol.id),
                schema_uri,
                workspace_revision: workspace_revision_view(prism.workspace_revision()),
                symbol,
                discovery,
                suggested_reads,
                read_context,
                edit_context,
                suggested_queries,
                relations,
                spec_cluster,
                spec_drift,
                lineage,
                co_change_neighbors,
                related_failures,
                blast_radius,
                validation_recipe,
                diagnostics: execution.diagnostics(),
                related_resources: dedupe_resource_link_views(related_resources),
            })
        })
    }

    pub(crate) fn search_resource_value(
        &self,
        session: Arc<SessionState>,
        uri: &str,
        query: &str,
    ) -> Result<SearchResourcePayload> {
        self.execute_traced_resource_read("search", uri, || {
            let schema_uri = schema_resource_uri("search");
            let prism = self.current_prism();
            let execution = QueryExecution::new(
                self.clone(),
                Arc::clone(&session),
                prism.clone(),
                self.begin_query_run(
                    session.as_ref(),
                    "read_resource",
                    "resource",
                    format!("prism://search/{query}"),
                ),
            );
            let strategy = parse_resource_query_param(uri, "strategy")
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "direct".to_string());
            let owner_kind = parse_resource_query_param(uri, "ownerKind")
                .or_else(|| parse_resource_query_param(uri, "owner_kind"));
            let kind = parse_resource_query_param(uri, "kind").filter(|value| !value.is_empty());
            let path = parse_resource_query_param(uri, "path").filter(|value| !value.is_empty());
            let module =
                parse_resource_query_param(uri, "module").filter(|value| !value.is_empty());
            let task_id = parse_resource_query_param(uri, "taskId")
                .or_else(|| parse_resource_query_param(uri, "task_id"))
                .filter(|value| !value.is_empty());
            let path_mode = parse_resource_query_param(uri, "pathMode")
                .or_else(|| parse_resource_query_param(uri, "path_mode"))
                .filter(|value| !value.is_empty());
            let structured_path = parse_resource_query_param(uri, "structuredPath")
                .or_else(|| parse_resource_query_param(uri, "structured_path"))
                .filter(|value| !value.is_empty());
            let top_level_only = parse_resource_query_param(uri, "topLevelOnly")
                .or_else(|| parse_resource_query_param(uri, "top_level_only"))
                .map(|value| match value.trim().to_ascii_lowercase().as_str() {
                    "true" | "1" | "yes" => Ok(true),
                    "false" | "0" | "no" => Ok(false),
                    other => Err(anyhow!("invalid topLevelOnly value `{other}`")),
                })
                .transpose()?;
            let prefer_callable_code = parse_resource_query_param(uri, "preferCallableCode")
                .or_else(|| parse_resource_query_param(uri, "prefer_callable_code"))
                .map(|value| match value.trim().to_ascii_lowercase().as_str() {
                    "true" | "1" | "yes" => Ok(true),
                    "false" | "0" | "no" => Ok(false),
                    other => Err(anyhow!("invalid preferCallableCode value `{other}`")),
                })
                .transpose()?;
            let prefer_editable_targets = parse_resource_query_param(uri, "preferEditableTargets")
                .or_else(|| parse_resource_query_param(uri, "prefer_editable_targets"))
                .map(|value| match value.trim().to_ascii_lowercase().as_str() {
                    "true" | "1" | "yes" => Ok(true),
                    "false" | "0" | "no" => Ok(false),
                    other => Err(anyhow!("invalid preferEditableTargets value `{other}`")),
                })
                .transpose()?;
            let prefer_behavioral_owners =
                parse_resource_query_param(uri, "preferBehavioralOwners")
                    .or_else(|| parse_resource_query_param(uri, "prefer_behavioral_owners"))
                    .map(|value| match value.trim().to_ascii_lowercase().as_str() {
                        "true" | "1" | "yes" => Ok(true),
                        "false" | "0" | "no" => Ok(false),
                        other => Err(anyhow!("invalid preferBehavioralOwners value `{other}`")),
                    })
                    .transpose()?;
            let include_inferred = parse_resource_query_param(uri, "includeInferred")
                .or_else(|| parse_resource_query_param(uri, "include_inferred"))
                .map(|value| match value.trim().to_ascii_lowercase().as_str() {
                    "true" | "1" | "yes" => Ok(true),
                    "false" | "0" | "no" => Ok(false),
                    other => Err(anyhow!("invalid includeInferred value `{other}`")),
                })
                .transpose()?
                .unwrap_or(true);
            let kind_filter = kind.as_deref().map(crate::parse_node_kind).transpose()?;
            let suggested_reads = owner_views_for_query(
                prism.as_ref(),
                query,
                owner_kind.as_deref(),
                kind_filter,
                path.as_deref(),
                crate::INSIGHT_LIMIT,
            )?;
            let mut suggested_reads = suggested_reads;
            compact_owner_candidate_excerpts(prism.as_ref(), &mut suggested_reads)?;
            let paged = paginate_items(
                execution.search(SearchArgs {
                    query: query.to_string(),
                    limit: Some(session.limits().max_result_nodes),
                    kind: kind.clone(),
                    path: path.clone(),
                    module: module.clone(),
                    task_id: task_id.clone(),
                    path_mode: path_mode.clone(),
                    strategy: Some(strategy.clone()),
                    structured_path: structured_path.clone(),
                    top_level_only,
                    prefer_callable_code,
                    prefer_editable_targets,
                    prefer_behavioral_owners,
                    owner_kind: owner_kind.clone(),
                    include_inferred: Some(include_inferred),
                })?,
                parse_resource_page(
                    uri,
                    DEFAULT_RESOURCE_PAGE_LIMIT,
                    session.limits().max_result_nodes,
                )?,
            );
            let discovery = paged
                .items
                .first()
                .map(|symbol| {
                    let id = NodeId::new(
                        symbol.id.crate_name.clone(),
                        symbol.id.path.clone(),
                        symbol.kind.clone(),
                    );
                    let mut bundle = discovery_bundle_view(prism.as_ref(), session.as_ref(), &id)?;
                    compact_discovery_bundle_candidate_excerpts(prism.as_ref(), &mut bundle)?;
                    Ok::<_, anyhow::Error>(bundle)
                })
                .transpose()?;
            let top_read_context = discovery.as_ref().map(|bundle| bundle.read_context.clone());
            let top_target = paged.items.first().map(|symbol| {
                NodeId::new(
                    symbol.id.crate_name.clone(),
                    symbol.id.path.clone(),
                    symbol.kind.clone(),
                )
            });
            let ambiguity = search_ambiguity_from_diagnostics(&execution.diagnostics());
            let suggested_queries =
                crate::search_suggested_queries(query, top_target.as_ref(), ambiguity.as_ref());
            let mut related_resources = vec![search_resource_view_link_with_options(
                query,
                Some(strategy.as_str()),
                owner_kind.as_deref(),
                kind.as_deref(),
                path.as_deref(),
                module.as_deref(),
                task_id.as_deref(),
                path_mode.as_deref(),
                structured_path.as_deref(),
                top_level_only,
                prefer_callable_code,
                prefer_editable_targets,
                prefer_behavioral_owners,
                Some(include_inferred),
            )];
            related_resources.extend(paged.items.iter().take(8).map(symbol_resource_view_link));
            related_resources.extend(
                suggested_reads
                    .iter()
                    .map(|candidate| symbol_resource_view_link(&candidate.symbol)),
            );
            related_resources.push(session_resource_view_link());
            related_resources.push(schema_resource_view_link("search"));
            related_resources.push(schemas_resource_view_link());
            Ok(SearchResourcePayload {
                uri: uri.to_string(),
                schema_uri,
                workspace_revision: workspace_revision_view(prism.workspace_revision()),
                query: query.to_string(),
                strategy,
                owner_kind,
                kind,
                path,
                module,
                task_id,
                path_mode,
                structured_path,
                top_level_only,
                prefer_callable_code,
                prefer_editable_targets,
                prefer_behavioral_owners,
                include_inferred,
                suggested_reads,
                results: paged.items,
                discovery,
                top_read_context,
                ambiguity,
                suggested_queries,
                page: paged.page,
                truncated: paged.truncated,
                diagnostics: execution.diagnostics(),
                related_resources: dedupe_resource_link_views(related_resources),
            })
        })
    }

    pub(crate) fn lineage_resource_value(
        &self,
        session: &SessionState,
        uri: &str,
        lineage: &LineageId,
    ) -> Result<LineageResourcePayload> {
        self.execute_traced_resource_read("lineage", uri, || {
            let schema_uri = schema_resource_uri("lineage");
            let prism = self.current_prism();
            let events = if let Some(workspace) = self.workspace_session() {
                workspace.load_lineage_history(lineage)?
            } else {
                prism.lineage_history(lineage)
            };
            let mut current_node_ids = prism.current_nodes_for_lineage(lineage);
            current_node_ids.sort_by(|left, right| {
                left.crate_name
                    .cmp(&right.crate_name)
                    .then_with(|| left.path.cmp(&right.path))
                    .then_with(|| left.kind.to_string().cmp(&right.kind.to_string()))
            });
            let current_nodes_truncated =
                current_node_ids.len() > session.limits().max_result_nodes;
            current_node_ids.truncate(session.limits().max_result_nodes);
            let current_nodes = symbol_views_for_ids(prism.as_ref(), current_node_ids.clone())?;
            let co_change_neighbors = current_node_ids
                .first()
                .map(|node| {
                    prism
                        .co_change_neighbors(node, 8)
                        .into_iter()
                        .map(co_change_view)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let paged_history = paginate_items(
                events.iter().map(lineage_event_view).collect::<Vec<_>>(),
                parse_resource_page(
                    uri,
                    DEFAULT_RESOURCE_PAGE_LIMIT,
                    session.limits().max_result_nodes,
                )?,
            );
            let mut related_resources = vec![
                session_resource_view_link(),
                lineage_resource_view_link(lineage.0.as_str()),
                schema_resource_view_link("lineage"),
                schemas_resource_view_link(),
            ];
            related_resources.extend(current_nodes.iter().map(symbol_resource_view_link));
            Ok(LineageResourcePayload {
                uri: uri.to_string(),
                schema_uri,
                lineage_id: lineage.0.to_string(),
                status: lineage_status(&events),
                current_nodes,
                current_nodes_truncated,
                history: paged_history.items,
                history_page: paged_history.page,
                truncated: paged_history.truncated || current_nodes_truncated,
                co_change_neighbors,
                diagnostics: Vec::new(),
                related_resources: dedupe_resource_link_views(related_resources),
            })
        })
    }

    pub(crate) fn task_resource_value(
        &self,
        session: &SessionState,
        uri: &str,
        task_id: &TaskId,
    ) -> Result<TaskResourcePayload> {
        self.execute_traced_resource_read("task", uri, || {
            let schema_uri = schema_resource_uri("task");
            let prism = self.current_prism();
            let replay =
                crate::load_task_replay(self.workspace_session_ref(), prism.as_ref(), task_id)
                    .unwrap_or_else(|_| prism.resume_task(task_id));
            let journal = crate::task_journal_view_from_replay(
                session,
                prism.as_ref(),
                replay.clone(),
                None,
                DEFAULT_TASK_JOURNAL_EVENT_LIMIT,
                DEFAULT_TASK_JOURNAL_MEMORY_LIMIT,
            )?;
            let paged = paginate_items(
                replay.events,
                parse_resource_page(
                    uri,
                    DEFAULT_RESOURCE_PAGE_LIMIT,
                    session.limits().max_result_nodes,
                )?,
            );
            let mut related_resources = vec![
                session_resource_view_link(),
                task_resource_view_link(replay.task.0.as_str()),
                schema_resource_view_link("task"),
                schemas_resource_view_link(),
            ];
            related_resources.extend(
                paged
                    .items
                    .iter()
                    .map(|event| event_resource_view_link(event.meta.id.0.as_str())),
            );
            related_resources.extend(paged.items.iter().flat_map(|event| {
                anchor_resource_view_links(prism.as_ref(), self.workspace_root(), &event.anchors)
            }));
            Ok(TaskResourcePayload {
                uri: uri.to_string(),
                schema_uri,
                task_id: replay.task.0.to_string(),
                journal,
                events: paged.items,
                page: paged.page,
                truncated: paged.truncated,
                related_resources: dedupe_resource_link_views(related_resources),
            })
        })
    }

    pub(crate) fn event_resource_value(&self, event_id: &EventId) -> Result<EventResourcePayload> {
        let uri = crate::event_resource_uri(event_id.0.as_str());
        self.execute_traced_resource_read("event", &uri, || {
            let schema_uri = schema_resource_uri("event");
            let prism = self.current_prism();
            let event = prism
                .outcome_event(event_id)
                .ok_or_else(|| anyhow!("unknown event `{}`", event_id.0))?;
            let mut related_resources = vec![
                session_resource_view_link(),
                event_resource_view_link(event_id.0.as_str()),
                schema_resource_view_link("event"),
                schemas_resource_view_link(),
            ];
            if let Some(task_id) = &event.meta.correlation {
                related_resources.push(task_resource_view_link(task_id.0.as_str()));
            }
            related_resources.extend(anchor_resource_view_links(
                prism.as_ref(),
                self.workspace_root(),
                &event.anchors,
            ));
            Ok(EventResourcePayload {
                uri: uri.clone(),
                schema_uri,
                event,
                related_resources: dedupe_resource_link_views(related_resources),
            })
        })
    }

    pub(crate) fn memory_resource_value(
        &self,
        session: &SessionState,
        memory_id: &MemoryId,
    ) -> Result<MemoryResourcePayload> {
        let uri = memory_resource_uri(&memory_id.0);
        self.execute_traced_resource_read("memory", &uri, || {
            let schema_uri = schema_resource_uri("memory");
            let prism = self.current_prism();
            let history_events = self
                .workspace_session()
                .map(|workspace| {
                    workspace.memory_events(&MemoryEventQuery {
                        memory_id: Some(memory_id.clone()),
                        focus: Vec::new(),
                        text: None,
                        limit: 20,
                        kinds: None,
                        actions: None,
                        scope: None,
                        task_id: None,
                        since: None,
                    })
                })
                .transpose()?
                .unwrap_or_default();
            let entry = session
                .notes
                .entry(memory_id)
                .or_else(|| {
                    history_events
                        .iter()
                        .find_map(|event| event.entry.as_ref().cloned())
                })
                .ok_or_else(|| anyhow!("unknown memory `{}`", memory_id.0))?;
            let task_id = entry
                .metadata
                .get("task_id")
                .and_then(|value| value.as_str())
                .map(ToOwned::to_owned)
                .or_else(|| {
                    history_events
                        .iter()
                        .find_map(|event| event.task_id.clone())
                });
            let history = history_events.into_iter().map(memory_event_view).collect();
            let mut related_resources = vec![
                session_resource_view_link(),
                memory_resource_view_link(&memory_id.0),
                schema_resource_view_link("memory"),
                schemas_resource_view_link(),
            ];
            if let Some(task_id) = &task_id {
                related_resources.push(task_resource_view_link(task_id));
            }
            related_resources.extend(anchor_resource_view_links(
                prism.as_ref(),
                self.workspace_root(),
                &entry.anchors,
            ));
            Ok(MemoryResourcePayload {
                uri: uri.clone(),
                schema_uri,
                memory: memory_entry_view(entry),
                task_id,
                history,
                related_resources: dedupe_resource_link_views(related_resources),
            })
        })
    }

    pub(crate) fn edge_resource_value(
        &self,
        session: &SessionState,
        edge_id: &EdgeId,
    ) -> Result<EdgeResourcePayload> {
        let uri = edge_resource_uri(&edge_id.0);
        self.execute_traced_resource_read("edge", &uri, || {
            let schema_uri = schema_resource_uri("edge");
            let record = session
                .inferred_edges
                .record(edge_id)
                .ok_or_else(|| anyhow!("unknown inferred edge `{}`", edge_id.0))?;
            let edge: InferredEdgeRecordView = inferred_edge_record_view(record);
            let mut related_resources = vec![
                session_resource_view_link(),
                edge_resource_view_link(&edge.id),
                schema_resource_view_link("edge"),
                schemas_resource_view_link(),
            ];
            if let Some(task_id) = &edge.task_id {
                related_resources.push(task_resource_view_link(task_id));
            }
            related_resources.push(symbol_resource_view_link_for_id(&NodeId::new(
                edge.edge.source.crate_name.clone(),
                edge.edge.source.path.clone(),
                edge.edge.source.kind.clone(),
            )));
            related_resources.push(symbol_resource_view_link_for_id(&NodeId::new(
                edge.edge.target.crate_name.clone(),
                edge.edge.target.path.clone(),
                edge.edge.target.kind.clone(),
            )));
            Ok(EdgeResourcePayload {
                uri: edge_resource_uri(&edge.id),
                schema_uri,
                edge,
                related_resources: dedupe_resource_link_views(related_resources),
            })
        })
    }
}

pub(crate) fn session_task_view(
    host: &QueryHost,
    _session: &SessionState,
    task: &crate::session_state::SessionTaskState,
) -> SessionTaskView {
    let prism = host.current_prism();
    let now = crate::current_timestamp();
    let replay =
        crate::load_task_replay(host.workspace_session_ref(), prism.as_ref(), &task.id).ok();
    let replay_event_count = replay.as_ref().map_or(0, |replay| replay.events.len());
    let coordination_task_id = task.coordination_task_id.clone().or_else(|| {
        task.id
            .0
            .starts_with("coord-task:")
            .then(|| task.id.0.to_string())
    });
    let coordination_task = coordination_task_id
        .as_ref()
        .and_then(|task_id| prism.coordination_task(&CoordinationTaskId::new(task_id.clone())));
    let blockers = coordination_task_id
        .as_ref()
        .map(|task_id| prism.blockers(&CoordinationTaskId::new(task_id.clone()), now))
        .unwrap_or_default();
    let heartbeat_advice = coordination_task_id.as_ref().and_then(|task_id| {
        task_heartbeat_advice(
            prism.as_ref(),
            &CoordinationTaskId::new(task_id.clone()),
            now,
        )
    });
    let context = session_task_context_summary(
        coordination_task_id.as_deref(),
        replay_event_count,
        coordination_task.is_some(),
        &blockers,
        heartbeat_advice.as_ref(),
    );

    SessionTaskView {
        task_id: task.id.0.to_string(),
        description: task.description.clone(),
        tags: task.tags.clone(),
        coordination_task_id,
        context_status: context.status.to_string(),
        context_summary: context.summary,
        next_action: context.next_action,
        repair_action: context.repair_action,
    }
}

struct SessionTaskContextSummary {
    status: &'static str,
    summary: String,
    next_action: String,
    repair_action: Option<SessionRepairActionView>,
}

fn session_task_context_summary(
    coordination_task_id: Option<&str>,
    replay_event_count: usize,
    has_coordination_task: bool,
    blockers: &[prism_coordination::TaskBlocker],
    heartbeat_advice: Option<&TaskHeartbeatAdvice>,
) -> SessionTaskContextSummary {
    if let Some(advice) = heartbeat_advice {
        return SessionTaskContextSummary {
            status: "heartbeat_due",
            summary: "Current task lease is nearing staleness and needs an authenticated heartbeat before other work continues."
                .to_string(),
            next_action: task_heartbeat_next_action(advice),
            repair_action: None,
        };
    }
    if blockers
        .iter()
        .any(|blocker| blocker.kind == prism_coordination::BlockerKind::StaleRevision)
    {
        let repair_action = coordination_task_id.map(|task_id| SessionRepairActionView {
            tool: "prism_task_brief".to_string(),
            input: json!({ "taskId": task_id }),
            label: "Inspect live blockers for this stale coordination task.".to_string(),
        });
        return SessionTaskContextSummary {
            status: "stale",
            summary: "Current task is bound to a stale coordination revision and may reflect older workspace state."
                .to_string(),
            next_action: "Refresh this task against the current workspace revision, then rerun prism_task_brief or prism.blockers(taskId).".to_string(),
            repair_action,
        };
    }
    if let Some(blocker) = blockers.first() {
        return SessionTaskContextSummary {
            status: "blocked",
            summary: format!("Current task is blocked: {}", blocker.summary),
            next_action: "Inspect the current task blockers before continuing; use prism.blockers(taskId) or prism_task_brief for full coordination detail.".to_string(),
            repair_action: None,
        };
    }
    if replay_event_count == 0 && !has_coordination_task {
        return SessionTaskContextSummary {
            status: "detached",
            summary: "Current task has no recorded replay history or live coordination binding and may be leftover session context."
                .to_string(),
            next_action: "Clear the current task if you are changing scope, or start a fresh task through the normal task-start flow.".to_string(),
            repair_action: Some(SessionRepairActionView {
                tool: "prism_mutate".to_string(),
                input: json!({
                    "action": "session_repair",
                    "input": {
                        "operation": "clear_current_task"
                    }
                }),
                label: "Clear the leftover current-task session binding.".to_string(),
            }),
        };
    }
    if replay_event_count == 0 && coordination_task_id.is_some() && !has_coordination_task {
        return SessionTaskContextSummary {
            status: "detached",
            summary: "Current task still references a coordination task that is no longer present in the live workspace context."
                .to_string(),
            next_action: "Rebind the session to a live coordination task, or clear the current task before starting new work.".to_string(),
            repair_action: Some(SessionRepairActionView {
                tool: "prism_mutate".to_string(),
                input: json!({
                    "action": "session_repair",
                    "input": {
                        "operation": "clear_current_task"
                    }
                }),
                label: "Clear the stale coordination-task session binding.".to_string(),
            }),
        };
    }
    SessionTaskContextSummary {
        status: "active",
        summary: if has_coordination_task {
            "Current task is bound to live coordination state.".to_string()
        } else if replay_event_count > 0 {
            format!(
                "Current task has recorded replay history ({} event{}).",
                replay_event_count,
                if replay_event_count == 1 { "" } else { "s" }
            )
        } else {
            "Current task is active in this session.".to_string()
        },
        next_action: "Continue with the current task, or open its task replay / task brief before switching scope.".to_string(),
        repair_action: None,
    }
}

pub(crate) fn contract_kind_label(kind: &crate::ContractKindView) -> &'static str {
    match kind {
        crate::ContractKindView::Interface => "interface",
        crate::ContractKindView::Behavioral => "behavioral",
        crate::ContractKindView::DataShape => "data_shape",
        crate::ContractKindView::DependencyBoundary => "dependency_boundary",
        crate::ContractKindView::Lifecycle => "lifecycle",
        crate::ContractKindView::Protocol => "protocol",
        crate::ContractKindView::Operational => "operational",
    }
}

pub(crate) fn contract_status_label(status: &crate::ContractStatusView) -> &'static str {
    match status {
        crate::ContractStatusView::Candidate => "candidate",
        crate::ContractStatusView::Active => "active",
        crate::ContractStatusView::Deprecated => "deprecated",
        crate::ContractStatusView::Retired => "retired",
    }
}

pub(crate) fn contract_scope_label(scope: &prism_js::ConceptScopeView) -> &'static str {
    match scope {
        prism_js::ConceptScopeView::Local => "local",
        prism_js::ConceptScopeView::Session => "session",
        prism_js::ConceptScopeView::Repo => "repo",
    }
}
