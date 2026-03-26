use anyhow::{anyhow, Result};
use prism_agent::EdgeId;
use prism_ir::{AnchorRef, EventId, LineageId, NodeId, TaskId};
use prism_js::LineageEventView;
use prism_memory::{MemoryId, OutcomeKind};

use crate::{
    anchor_resource_view_links, blast_radius_view, co_change_view, convert_node_id,
    dedupe_resource_link_views, edge_resource_uri, edge_resource_view_link,
    event_resource_view_link, inferred_edge_record_view, lineage_resource_view_link,
    lineage_status, memory_entry_view, memory_resource_uri, memory_resource_view_link,
    paginate_items, parse_resource_page, relations_view, resource_link_view,
    resource_schema_catalog_entries, schema_resource_uri, schema_resource_view_link,
    schemas_resource_uri, schemas_resource_view_link, search_resource_view_link,
    session_resource_uri, session_resource_view_link, symbol_for, symbol_resource_uri,
    symbol_resource_view_link, symbol_resource_view_link_for_id, symbol_view, symbol_views_for_ids,
    task_resource_view_link, task_resource_view_links_from_events, validation_recipe_view_with,
    EdgeResourcePayload, EntrypointsResourcePayload, EventResourcePayload, InferredEdgeRecordView,
    LineageResourcePayload, MemoryResourcePayload, NodeIdInput, QueryExecution, QueryHost,
    ResourceSchemaCatalogPayload, SearchArgs, SearchResourcePayload, SessionLimitsView,
    SessionResourcePayload, SessionTaskView, SessionView, SymbolResourcePayload,
    TaskResourcePayload, DEFAULT_RESOURCE_PAGE_LIMIT, ENTRYPOINTS_URI,
};

impl QueryHost {
    pub(crate) fn session_view(&self) -> Result<SessionView> {
        self.refresh_workspace()?;
        let limits = self.session.limits();
        Ok(SessionView {
            workspace_root: self
                .workspace
                .as_ref()
                .map(|workspace| workspace.root().display().to_string()),
            current_task: self
                .session
                .current_task_state()
                .map(|task| SessionTaskView {
                    task_id: task.id.0.to_string(),
                    description: task.description,
                    tags: task.tags,
                }),
            limits: SessionLimitsView {
                max_result_nodes: limits.max_result_nodes,
                max_call_graph_depth: limits.max_call_graph_depth,
                max_output_json_bytes: limits.max_output_json_bytes,
            },
        })
    }

    pub(crate) fn session_resource_value(&self) -> Result<SessionResourcePayload> {
        let schema_uri = schema_resource_uri("session");
        let session = self.session_view()?;
        let mut related_resources = vec![
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
            limits: session.limits,
            related_resources: dedupe_resource_link_views(related_resources),
        })
    }

    pub(crate) fn schemas_resource_value(&self) -> ResourceSchemaCatalogPayload {
        let mut related_resources = vec![
            schemas_resource_view_link(),
            schema_resource_view_link("schemas"),
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

    pub(crate) fn task_metadata(&self, task_id: &TaskId) -> (Option<String>, Vec<String>) {
        if let Some(task) = self.session.current_task_state() {
            if task.id == *task_id {
                return (task.description, task.tags);
            }
        }

        let replay = self.current_prism().resume_task(task_id);
        let description = replay
            .events
            .iter()
            .find(|event| event.kind == OutcomeKind::PlanCreated)
            .map(|event| event.summary.clone());
        let tags = replay
            .events
            .iter()
            .find(|event| event.kind == OutcomeKind::PlanCreated)
            .and_then(|event| event.metadata.get("tags"))
            .and_then(|value| value.as_array())
            .map(|values| {
                values
                    .iter()
                    .filter_map(|value| value.as_str().map(ToOwned::to_owned))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        (description, tags)
    }

    pub(crate) fn entrypoints_resource_value(
        &self,
        uri: &str,
    ) -> Result<EntrypointsResourcePayload> {
        self.refresh_workspace()?;
        let schema_uri = schema_resource_uri("entrypoints");
        let prism = self.current_prism();
        let execution = QueryExecution::new(self.clone(), prism);
        let paged = paginate_items(
            execution.entrypoints()?,
            parse_resource_page(
                uri,
                DEFAULT_RESOURCE_PAGE_LIMIT,
                self.session.limits().max_result_nodes,
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

    pub(crate) fn symbol_resource_value(&self, id: &NodeId) -> Result<SymbolResourcePayload> {
        self.refresh_workspace()?;
        let schema_uri = schema_resource_uri("symbol");
        let prism = self.current_prism();
        let execution = QueryExecution::new(self.clone(), prism.clone());
        let symbol = symbol_for(prism.as_ref(), id)?;
        let symbol = symbol_view(prism.as_ref(), &symbol)?;
        let relations = relations_view(prism.as_ref(), self.session.as_ref(), id)?;
        let lineage = crate::lineage_view(prism.as_ref(), id)?;
        let co_change_neighbors = prism
            .co_change_neighbors(id, 8)
            .into_iter()
            .map(co_change_view)
            .collect::<Vec<_>>();
        let related_failures = prism.related_failures(id);
        let blast_radius = blast_radius_view(prism.as_ref(), self.session.as_ref(), id);
        let validation_recipe =
            validation_recipe_view_with(prism.as_ref(), self.session.as_ref(), id);
        let mut related_resources = vec![
            session_resource_view_link(),
            symbol_resource_view_link(&symbol),
            schema_resource_view_link("symbol"),
            schemas_resource_view_link(),
        ];
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
        Ok(SymbolResourcePayload {
            uri: symbol_resource_uri(&symbol.id),
            schema_uri,
            symbol,
            relations,
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
        uri: &str,
        query: &str,
    ) -> Result<SearchResourcePayload> {
        self.refresh_workspace()?;
        let schema_uri = schema_resource_uri("search");
        let execution = QueryExecution::new(self.clone(), self.current_prism());
        let paged = paginate_items(
            execution.search(SearchArgs {
                query: query.to_string(),
                limit: Some(self.session.limits().max_result_nodes),
                kind: None,
                path: None,
                include_inferred: None,
            })?,
            parse_resource_page(
                uri,
                DEFAULT_RESOURCE_PAGE_LIMIT,
                self.session.limits().max_result_nodes,
            )?,
        );
        let mut related_resources = vec![
            session_resource_view_link(),
            search_resource_view_link(query),
            schema_resource_view_link("search"),
            schemas_resource_view_link(),
        ];
        related_resources.extend(paged.items.iter().take(8).map(symbol_resource_view_link));
        Ok(SearchResourcePayload {
            uri: uri.to_string(),
            schema_uri,
            query: query.to_string(),
            results: paged.items,
            page: paged.page,
            truncated: paged.truncated,
            diagnostics: execution.diagnostics(),
            related_resources: dedupe_resource_link_views(related_resources),
        })
    }

    pub(crate) fn lineage_resource_value(
        &self,
        uri: &str,
        lineage: &LineageId,
    ) -> Result<LineageResourcePayload> {
        self.refresh_workspace()?;
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
        let current_nodes_truncated =
            current_node_ids.len() > self.session.limits().max_result_nodes;
        current_node_ids.truncate(self.session.limits().max_result_nodes);
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
            events
                .iter()
                .map(|event| LineageEventView {
                    event_id: event.meta.id.0.to_string(),
                    ts: event.meta.ts,
                    kind: format!("{:?}", event.kind),
                    confidence: event.confidence,
                })
                .collect::<Vec<_>>(),
            parse_resource_page(
                uri,
                DEFAULT_RESOURCE_PAGE_LIMIT,
                self.session.limits().max_result_nodes,
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
        uri: &str,
        task_id: &TaskId,
    ) -> Result<TaskResourcePayload> {
        self.refresh_workspace()?;
        let schema_uri = schema_resource_uri("task");
        let prism = self.current_prism();
        let replay = prism.resume_task(task_id);
        let paged = paginate_items(
            replay.events,
            parse_resource_page(
                uri,
                DEFAULT_RESOURCE_PAGE_LIMIT,
                self.session.limits().max_result_nodes,
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
            events: paged.items,
            page: paged.page,
            truncated: paged.truncated,
            related_resources: dedupe_resource_link_views(related_resources),
        })
    }

    pub(crate) fn event_resource_value(&self, event_id: &EventId) -> Result<EventResourcePayload> {
        self.refresh_workspace()?;
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
        memory_id: &MemoryId,
    ) -> Result<MemoryResourcePayload> {
        self.refresh_workspace()?;
        let schema_uri = schema_resource_uri("memory");
        let entry = self
            .session
            .notes
            .entry(memory_id)
            .ok_or_else(|| anyhow!("unknown memory `{}`", memory_id.0))?;
        let task_id = entry
            .metadata
            .get("task_id")
            .and_then(|value| value.as_str())
            .map(ToOwned::to_owned);
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
            related_resources: dedupe_resource_link_views(related_resources),
        })
    }

    pub(crate) fn edge_resource_value(&self, edge_id: &EdgeId) -> Result<EdgeResourcePayload> {
        self.refresh_workspace()?;
        let schema_uri = schema_resource_uri("edge");
        let record = self
            .session
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
        related_resources.push(symbol_resource_view_link_for_id(&convert_node_id(
            NodeIdInput {
                crate_name: edge.edge.source.crate_name.clone(),
                path: edge.edge.source.path.clone(),
                kind: edge.edge.source.kind.to_string(),
            },
        )?));
        related_resources.push(symbol_resource_view_link_for_id(&convert_node_id(
            NodeIdInput {
                crate_name: edge.edge.target.crate_name.clone(),
                path: edge.edge.target.path.clone(),
                kind: edge.edge.target.kind.to_string(),
            },
        )?));
        Ok(EdgeResourcePayload {
            uri: edge_resource_uri(&edge.id),
            schema_uri,
            edge,
            related_resources: dedupe_resource_link_views(related_resources),
        })
    }
}
