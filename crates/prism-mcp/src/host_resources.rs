use anyhow::{anyhow, Result};
use prism_agent::EdgeId;
use prism_ir::{AnchorRef, EventId, LineageId, NodeId, TaskId};
use prism_memory::{MemoryEventQuery, MemoryId};
use std::sync::Arc;

use crate::{
    anchor_resource_view_links, capabilities_resource_value, capabilities_resource_view_link,
    co_change_view, compact_discovery_bundle_candidate_excerpts, compact_owner_candidate_excerpts,
    dedupe_resource_link_views, derive_task_metadata, discovery_bundle_view, edge_resource_uri,
    edge_resource_view_link, event_resource_view_link, inferred_edge_record_view,
    lineage_event_view, lineage_resource_view_link, lineage_status, memory_entry_view,
    memory_event_view, memory_resource_uri, memory_resource_view_link, owner_views_for_query,
    paginate_items, parse_resource_page, parse_resource_query_param, resource_link_view,
    resource_schema_catalog_entries, schema_resource_uri, schema_resource_view_link,
    schemas_resource_uri, schemas_resource_view_link, search_ambiguity_from_diagnostics,
    search_resource_view_link_with_options, session_resource_uri, session_resource_view_link,
    symbol_for, symbol_resource_uri, symbol_resource_view_link, symbol_resource_view_link_for_id,
    symbol_view, symbol_views_for_ids, task_journal_view, task_resource_view_link,
    task_resource_view_links_from_events, tool_schemas_resource_value,
    tool_schemas_resource_view_link, workspace_revision_view, CapabilitiesResourcePayload,
    CoordinationFeaturesView, EdgeResourcePayload, EntrypointsResourcePayload,
    EventResourcePayload, FeatureFlagsView, InferredEdgeRecordView, LineageResourcePayload,
    MemoryResourcePayload, QueryExecution, QueryHost, ResourceSchemaCatalogPayload, SearchArgs,
    SearchResourcePayload, SessionLimitsView, SessionResourcePayload, SessionState,
    SessionTaskView, SessionView, SymbolResourcePayload, TaskResourcePayload,
    DEFAULT_RESOURCE_PAGE_LIMIT, DEFAULT_TASK_JOURNAL_EVENT_LIMIT,
    DEFAULT_TASK_JOURNAL_MEMORY_LIMIT, ENTRYPOINTS_URI,
};

impl QueryHost {
    pub(crate) fn session_view_without_refresh(&self, session: &SessionState) -> SessionView {
        let limits = session.limits();
        SessionView {
            workspace_root: self
                .workspace
                .as_ref()
                .map(|workspace| workspace.root().display().to_string()),
            current_task: session.current_task_state().map(|task| SessionTaskView {
                task_id: task.id.0.to_string(),
                description: task.description,
                tags: task.tags,
            }),
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

    pub(crate) fn session_view(&self, session: &SessionState) -> Result<SessionView> {
        self.refresh_workspace_for_query()?;
        Ok(self.session_view_without_refresh(session))
    }

    pub(crate) fn session_resource_value(
        &self,
        session: &SessionState,
    ) -> Result<SessionResourcePayload> {
        let schema_uri = schema_resource_uri("session");
        let session = self.session_view(session)?;
        let mut related_resources = vec![
            capabilities_resource_view_link(),
            session_resource_view_link(),
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
            uri: session_resource_uri(),
            schema_uri,
            workspace_root: session.workspace_root,
            current_task: session.current_task,
            current_agent: session.current_agent,
            limits: session.limits,
            features: session.features,
            related_resources: dedupe_resource_link_views(related_resources),
        })
    }

    pub(crate) fn capabilities_resource_value(&self) -> Result<CapabilitiesResourcePayload> {
        capabilities_resource_value(self)
    }

    pub(crate) fn schemas_resource_value(&self) -> ResourceSchemaCatalogPayload {
        let mut related_resources = vec![
            capabilities_resource_view_link(),
            schemas_resource_view_link(),
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

    pub(crate) fn task_metadata(
        &self,
        session: &SessionState,
        task_id: &TaskId,
    ) -> (Option<String>, Vec<String>) {
        let replay = self.current_prism().resume_task(task_id);
        derive_task_metadata(
            session.current_task_state().as_ref(),
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
        self.refresh_workspace_for_query()?;
        let schema_uri = schema_resource_uri("entrypoints");
        let prism = self.current_prism();
        let execution = QueryExecution::new(
            self.clone(),
            Arc::clone(&session),
            prism,
            self.begin_query_run(session.as_ref(), "resource", "prism://entrypoints"),
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
    }

    pub(crate) fn symbol_resource_value(
        &self,
        session: Arc<SessionState>,
        id: &NodeId,
    ) -> Result<SymbolResourcePayload> {
        self.refresh_workspace_for_query()?;
        let schema_uri = schema_resource_uri("symbol");
        let prism = self.current_prism();
        let execution = QueryExecution::new(
            self.clone(),
            Arc::clone(&session),
            prism.clone(),
            self.begin_query_run(
                session.as_ref(),
                "resource",
                format!("prism://symbol/{}", id.path),
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
    }

    pub(crate) fn search_resource_value(
        &self,
        session: Arc<SessionState>,
        uri: &str,
        query: &str,
    ) -> Result<SearchResourcePayload> {
        self.refresh_workspace_for_query()?;
        let schema_uri = schema_resource_uri("search");
        let prism = self.current_prism();
        let execution = QueryExecution::new(
            self.clone(),
            Arc::clone(&session),
            prism.clone(),
            self.begin_query_run(
                session.as_ref(),
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
        let module = parse_resource_query_param(uri, "module").filter(|value| !value.is_empty());
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
        let prefer_behavioral_owners = parse_resource_query_param(uri, "preferBehavioralOwners")
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
    }

    pub(crate) fn lineage_resource_value(
        &self,
        session: &SessionState,
        uri: &str,
        lineage: &LineageId,
    ) -> Result<LineageResourcePayload> {
        self.refresh_workspace_for_query()?;
        let schema_uri = schema_resource_uri("lineage");
        let prism = self.current_prism();
        let history = prism.history_snapshot();
        let events = prism.lineage_history(lineage);
        let mut current_node_ids = history
            .node_to_lineage
            .into_iter()
            .filter_map(|(node, current)| (current == *lineage).then_some(node))
            .collect::<Vec<_>>();
        current_node_ids.sort_by(|left, right| {
            left.crate_name
                .cmp(&right.crate_name)
                .then_with(|| left.path.cmp(&right.path))
                .then_with(|| left.kind.to_string().cmp(&right.kind.to_string()))
        });
        let current_nodes_truncated = current_node_ids.len() > session.limits().max_result_nodes;
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
    }

    pub(crate) fn task_resource_value(
        &self,
        session: &SessionState,
        uri: &str,
        task_id: &TaskId,
    ) -> Result<TaskResourcePayload> {
        self.refresh_workspace_for_query()?;
        let schema_uri = schema_resource_uri("task");
        let prism = self.current_prism();
        let replay = prism.resume_task(task_id);
        let journal = task_journal_view(
            session,
            prism.as_ref(),
            task_id,
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
        related_resources.extend(
            paged
                .items
                .iter()
                .flat_map(|event| anchor_resource_view_links(&event.anchors)),
        );
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
    }

    pub(crate) fn event_resource_value(&self, event_id: &EventId) -> Result<EventResourcePayload> {
        self.refresh_workspace_for_query()?;
        let schema_uri = schema_resource_uri("event");
        let event = self
            .current_prism()
            .outcome_memory()
            .event(event_id)
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
        related_resources.extend(anchor_resource_view_links(&event.anchors));
        Ok(EventResourcePayload {
            uri: crate::event_resource_uri(event_id.0.as_str()),
            schema_uri,
            event,
            related_resources: dedupe_resource_link_views(related_resources),
        })
    }

    pub(crate) fn memory_resource_value(
        &self,
        session: &SessionState,
        memory_id: &MemoryId,
    ) -> Result<MemoryResourcePayload> {
        self.refresh_workspace_for_query()?;
        let schema_uri = schema_resource_uri("memory");
        let entry = session
            .notes
            .entry(memory_id)
            .ok_or_else(|| anyhow!("unknown memory `{}`", memory_id.0))?;
        let task_id = entry
            .metadata
            .get("task_id")
            .and_then(|value| value.as_str())
            .map(ToOwned::to_owned);
        let history = self
            .workspace
            .as_ref()
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
            .unwrap_or_default()
            .into_iter()
            .map(memory_event_view)
            .collect();
        let mut related_resources = vec![
            session_resource_view_link(),
            memory_resource_view_link(&memory_id.0),
            schema_resource_view_link("memory"),
            schemas_resource_view_link(),
        ];
        if let Some(task_id) = &task_id {
            related_resources.push(task_resource_view_link(task_id));
        }
        related_resources.extend(anchor_resource_view_links(&entry.anchors));
        Ok(MemoryResourcePayload {
            uri: memory_resource_uri(&memory_id.0),
            schema_uri,
            memory: memory_entry_view(entry),
            task_id,
            history,
            related_resources: dedupe_resource_link_views(related_resources),
        })
    }

    pub(crate) fn edge_resource_value(
        &self,
        session: &SessionState,
        edge_id: &EdgeId,
    ) -> Result<EdgeResourcePayload> {
        self.refresh_workspace_for_query()?;
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
    }
}
