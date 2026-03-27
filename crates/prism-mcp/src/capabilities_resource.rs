use rmcp::model::ProtocolVersion;

use crate::{
    capabilities_resource_uri, capabilities_resource_view_link, resource_example_uri,
    resource_link_view, resource_schema_catalog_entries, schema_resource_uri,
    schema_resource_view_link, search_resource_view_link_with_options, session_resource_view_link,
    tool_schema_catalog_entries, tool_schema_resource_uri, tool_schemas_resource_view_link,
    workspace_revision_view, CapabilitiesBuildInfoView, CapabilitiesResourcePayload,
    FeatureFlagsView, PrismMcpFeatures, QueryHost, QueryMethodCapabilityView,
    ResourceCapabilityView, ResourceTemplateCapabilityView, ToolCapabilityView, API_REFERENCE_URI,
    CAPABILITIES_URI, EDGE_RESOURCE_TEMPLATE_URI, ENTRYPOINTS_RESOURCE_TEMPLATE_URI,
    EVENT_RESOURCE_TEMPLATE_URI, LINEAGE_RESOURCE_TEMPLATE_URI, MEMORY_RESOURCE_TEMPLATE_URI,
    SCHEMAS_URI, SEARCH_RESOURCE_TEMPLATE_URI, SESSION_URI, SYMBOL_RESOURCE_TEMPLATE_URI,
    TASK_RESOURCE_TEMPLATE_URI, TOOL_SCHEMAS_URI, TOOL_SCHEMA_RESOURCE_TEMPLATE_URI,
};

pub(crate) fn capabilities_resource_value(
    host: &QueryHost,
) -> anyhow::Result<CapabilitiesResourcePayload> {
    host.refresh_workspace()?;
    let prism = host.current_prism();
    let mut related_resources = vec![
        capabilities_resource_view_link(),
        session_resource_view_link(),
        schema_resource_view_link("capabilities"),
        schema_resource_view_link("session"),
        schema_resource_view_link("schemas"),
        tool_schemas_resource_view_link(),
        resource_link_view(
            API_REFERENCE_URI.to_string(),
            "PRISM API Reference",
            "TypeScript query surface, d.ts-style contract, and usage recipes",
        ),
        search_resource_view_link_with_options(
            "read context",
            Some("behavioral"),
            Some("read"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        ),
    ];
    related_resources.extend(
        resource_schema_catalog_entries()
            .into_iter()
            .take(4)
            .map(|entry| schema_resource_view_link(&entry.resource_kind)),
    );
    Ok(CapabilitiesResourcePayload {
        uri: capabilities_resource_uri(),
        schema_uri: schema_resource_uri("capabilities"),
        build: CapabilitiesBuildInfoView {
            server_name: env!("CARGO_PKG_NAME").to_string(),
            server_version: env!("CARGO_PKG_VERSION").to_string(),
            protocol_version: ProtocolVersion::LATEST.as_str().to_string(),
            workspace_revision: workspace_revision_view(prism.workspace_revision()),
            api_reference_uri: API_REFERENCE_URI.to_string(),
        },
        features: FeatureFlagsView {
            mode: host.features.mode_label().to_string(),
            coordination: crate::CoordinationFeaturesView {
                workflow: host.features.coordination.workflow,
                claims: host.features.coordination.claims,
                artifacts: host.features.coordination.artifacts,
            },
            internal_developer: host.features.internal_developer,
        },
        query_methods: query_method_capabilities(&host.features),
        resources: resource_capabilities(),
        resource_templates: resource_template_capabilities(),
        tools: tool_schema_catalog_entries()
            .into_iter()
            .map(|entry| ToolCapabilityView {
                name: entry.tool_name,
                description: entry.description,
                schema_uri: entry.schema_uri,
                example_input: entry.example_input,
            })
            .collect(),
        related_resources: crate::dedupe_resource_link_views(related_resources),
    })
}

fn query_method_capabilities(features: &PrismMcpFeatures) -> Vec<QueryMethodCapabilityView> {
    query_method_specs()
        .into_iter()
        .filter(|(name, _, _, _)| features.query_method_visible(name))
        .map(
            |(name, group, feature_gate, description)| QueryMethodCapabilityView {
                name: name.to_string(),
                enabled: features.disabled_query_group(name).is_none(),
                group: group.to_string(),
                feature_gate: feature_gate.map(str::to_string),
                description: description.to_string(),
            },
        )
        .collect()
}

fn query_method_specs() -> Vec<(
    &'static str,
    &'static str,
    Option<&'static str>,
    &'static str,
)> {
    vec![
        (
            "symbol",
            "core",
            None,
            "Resolve the best symbol match for a query string.",
        ),
        (
            "symbols",
            "core",
            None,
            "Return all direct symbol matches for a query string.",
        ),
        (
            "search",
            "core",
            None,
            "Search symbols directly or via owner-biased behavioral ranking.",
        ),
        (
            "searchText",
            "core",
            None,
            "Search workspace file text with exact match spans, snippets, and path filters.",
        ),
        (
            "tools",
            "core",
            None,
            "List PRISM MCP tools with schema URIs, descriptions, and example inputs.",
        ),
        (
            "tool",
            "core",
            None,
            "Inspect one PRISM MCP tool schema, its action variants, required fields, and example input.",
        ),
        (
            "entrypoints",
            "core",
            None,
            "List indexed workspace entrypoints.",
        ),
        (
            "fileRead",
            "core",
            None,
            "Read an exact workspace file slice by path and line range.",
        ),
        (
            "fileAround",
            "core",
            None,
            "Read a bounded workspace file slice around one line.",
        ),
        (
            "lineage",
            "core",
            None,
            "Read lineage history and status for a target.",
        ),
        (
            "coChangeNeighbors",
            "core",
            None,
            "List co-change neighbors for a target.",
        ),
        (
            "relatedFailures",
            "core",
            None,
            "List recent failure events related to a target.",
        ),
        (
            "blastRadius",
            "core",
            None,
            "Estimate impact and likely validations for a target.",
        ),
        (
            "validationRecipe",
            "core",
            None,
            "Suggest validations and checks for a target.",
        ),
        (
            "readContext",
            "core",
            None,
            "Build a semantic read bundle for a target.",
        ),
        (
            "editContext",
            "core",
            None,
            "Build an edit-focused bundle with write paths and risk.",
        ),
        (
            "validationContext",
            "core",
            None,
            "Build a validation-focused bundle with test owners, failures, and checks.",
        ),
        (
            "recentChangeContext",
            "core",
            None,
            "Build a recent-change bundle with outcomes, co-change signals, and lineage.",
        ),
        (
            "discoveryBundle",
            "core",
            None,
            "Build a composite discovery bundle for a target in one host call.",
        ),
        (
            "nextReads",
            "core",
            None,
            "Return the strongest read-oriented next candidates for a target.",
        ),
        (
            "whereUsed",
            "core",
            None,
            "Return direct or behavioral usage paths for a target.",
        ),
        (
            "entrypointsFor",
            "core",
            None,
            "Find entrypoints that can reach a target through call paths.",
        ),
        ("specFor", "core", None, "Resolve spec links for a target."),
        (
            "implementationFor",
            "core",
            None,
            "Resolve direct or owner-biased implementations.",
        ),
        (
            "owners",
            "core",
            None,
            "Return owner candidates biased by read, write, persist, or test paths.",
        ),
        (
            "specCluster",
            "core",
            None,
            "Group a spec with implementations, tests, and owner paths.",
        ),
        (
            "explainDrift",
            "core",
            None,
            "Explain likely spec drift and next reads.",
        ),
        (
            "resumeTask",
            "memory",
            None,
            "Replay recorded task outcomes.",
        ),
        (
            "taskJournal",
            "memory",
            None,
            "Summarize task lifecycle, events, and related memory.",
        ),
        (
            "memoryRecall",
            "memory",
            None,
            "Recall anchored session memory.",
        ),
        (
            "memoryOutcomes",
            "memory",
            None,
            "Query outcome history with filters.",
        ),
        (
            "curatorJobs",
            "curator",
            None,
            "Inspect recorded curator jobs.",
        ),
        ("curatorJob", "curator", None, "Inspect one curator job."),
        (
            "plan",
            "coordination",
            Some("workflow"),
            "Read a coordination plan.",
        ),
        (
            "coordinationTask",
            "coordination",
            Some("workflow"),
            "Read one coordination task.",
        ),
        (
            "readyTasks",
            "coordination",
            Some("workflow"),
            "List ready coordination tasks.",
        ),
        (
            "blockers",
            "coordination",
            Some("workflow"),
            "Explain why a coordination task is blocked.",
        ),
        (
            "policyViolations",
            "coordination",
            Some("workflow"),
            "List recorded policy violations.",
        ),
        (
            "taskBlastRadius",
            "coordination",
            Some("workflow"),
            "Estimate impact for a coordination task.",
        ),
        (
            "taskValidationRecipe",
            "coordination",
            Some("workflow"),
            "Suggest validations for a coordination task.",
        ),
        (
            "taskRisk",
            "coordination",
            Some("workflow"),
            "Summarize task risk and review requirements.",
        ),
        (
            "taskIntent",
            "coordination",
            Some("workflow"),
            "Read coordination task intent and drift hints.",
        ),
        (
            "claims",
            "coordination",
            Some("claims"),
            "List active claims for anchors.",
        ),
        (
            "conflicts",
            "coordination",
            Some("claims"),
            "List claim conflicts for anchors.",
        ),
        (
            "simulateClaim",
            "coordination",
            Some("claims"),
            "Preview claim conflicts before acquiring one.",
        ),
        (
            "pendingReviews",
            "coordination",
            Some("artifacts"),
            "List pending review artifacts.",
        ),
        (
            "artifacts",
            "coordination",
            Some("artifacts"),
            "List artifacts for a coordination task.",
        ),
        (
            "artifactRisk",
            "coordination",
            Some("artifacts"),
            "Read artifact risk and missing validations.",
        ),
        (
            "changedFiles",
            "core",
            None,
            "List recently changed files with semantic patch summaries and symbol counts.",
        ),
        (
            "changedSymbols",
            "core",
            None,
            "List recently changed symbols for one file with exact locations and local excerpts where available.",
        ),
        (
            "recentPatches",
            "core",
            None,
            "Inspect recent semantic patch events, optionally narrowed by target, task, or path.",
        ),
        (
            "diffFor",
            "core",
            None,
            "Inspect exact semantic changed hunks for one target, with lineage-aware narrowing and local excerpts.",
        ),
        (
            "focusedBlock",
            "core",
            None,
            "Fetch the exact local block around one target, preferring a focused edit slice and falling back to a bounded excerpt.",
        ),
        (
            "taskChanges",
            "core",
            None,
            "Inspect recent semantic patch events for one task.",
        ),
        (
            "runtimeStatus",
            "internal",
            Some("internal_developer"),
            "Inspect the MCP daemon status, health, process counts, and runtime file paths for this workspace.",
        ),
        (
            "runtimeLogs",
            "internal",
            Some("internal_developer"),
            "Read recent structured daemon log events with level, target, and text filtering.",
        ),
        (
            "runtimeTimeline",
            "internal",
            Some("internal_developer"),
            "Read a startup and refresh-focused runtime timeline from recent daemon log events.",
        ),
        (
            "queryLog",
            "internal",
            Some("internal_developer"),
            "List recent PRISM queries with timing, diagnostics, and truncation metadata.",
        ),
        (
            "slowQueries",
            "internal",
            Some("internal_developer"),
            "List slow PRISM queries with duration-based filtering and sorting.",
        ),
        (
            "queryTrace",
            "internal",
            Some("internal_developer"),
            "Inspect the phase-by-phase trace for one recorded PRISM query.",
        ),
        (
            "diagnostics",
            "core",
            None,
            "Return diagnostics gathered during the current query.",
        ),
    ]
}

fn resource_capabilities() -> Vec<ResourceCapabilityView> {
    vec![
        ResourceCapabilityView {
            name: "PRISM API Reference".to_string(),
            uri: API_REFERENCE_URI.to_string(),
            mime_type: "text/markdown".to_string(),
            description: "TypeScript query surface, d.ts-style contract, and usage recipes."
                .to_string(),
            schema_uri: None,
            example_uri: Some(API_REFERENCE_URI.to_string()),
        },
        ResourceCapabilityView {
            name: "PRISM Capabilities".to_string(),
            uri: CAPABILITIES_URI.to_string(),
            mime_type: "application/json".to_string(),
            description:
                "Canonical capability map for query methods, resources, features, and build info."
                    .to_string(),
            schema_uri: Some(schema_resource_uri("capabilities")),
            example_uri: resource_example_uri("capabilities"),
        },
        ResourceCapabilityView {
            name: "PRISM Session".to_string(),
            uri: SESSION_URI.to_string(),
            mime_type: "application/json".to_string(),
            description: "Active workspace root, task context, limits, and feature flags."
                .to_string(),
            schema_uri: Some(schema_resource_uri("session")),
            example_uri: resource_example_uri("session"),
        },
        ResourceCapabilityView {
            name: "PRISM Resource Schemas".to_string(),
            uri: SCHEMAS_URI.to_string(),
            mime_type: "application/json".to_string(),
            description: "Catalog of JSON Schemas for structured PRISM resource payloads."
                .to_string(),
            schema_uri: Some(schema_resource_uri("schemas")),
            example_uri: resource_example_uri("schemas"),
        },
        ResourceCapabilityView {
            name: "PRISM Tool Schemas".to_string(),
            uri: TOOL_SCHEMAS_URI.to_string(),
            mime_type: "application/json".to_string(),
            description: "Catalog of JSON Schemas for PRISM MCP tool inputs.".to_string(),
            schema_uri: Some(schema_resource_uri("tool-schemas")),
            example_uri: resource_example_uri("tool-schemas"),
        },
    ]
}

fn resource_template_capabilities() -> Vec<ResourceTemplateCapabilityView> {
    vec![
        ResourceTemplateCapabilityView {
            name: "PRISM Entrypoints Page".to_string(),
            uri_template: ENTRYPOINTS_RESOURCE_TEMPLATE_URI.to_string(),
            mime_type: "application/json".to_string(),
            description: "Read workspace entrypoints with optional pagination.".to_string(),
            example_uri: resource_example_uri("entrypoints"),
        },
        ResourceTemplateCapabilityView {
            name: "PRISM Resource Schema".to_string(),
            uri_template: crate::SCHEMA_RESOURCE_TEMPLATE_URI.to_string(),
            mime_type: "application/schema+json".to_string(),
            description: "Read a JSON Schema for a structured PRISM resource kind.".to_string(),
            example_uri: Some(schema_resource_uri("search")),
        },
        ResourceTemplateCapabilityView {
            name: "PRISM Tool Schema".to_string(),
            uri_template: TOOL_SCHEMA_RESOURCE_TEMPLATE_URI.to_string(),
            mime_type: "application/schema+json".to_string(),
            description: "Read a JSON Schema for a PRISM MCP tool input payload.".to_string(),
            example_uri: Some(tool_schema_resource_uri("prism_query")),
        },
        ResourceTemplateCapabilityView {
            name: "PRISM Search".to_string(),
            uri_template: SEARCH_RESOURCE_TEMPLATE_URI.to_string(),
            mime_type: "application/json".to_string(),
            description: "Read structured search results and diagnostics for a query.".to_string(),
            example_uri: resource_example_uri("search"),
        },
        ResourceTemplateCapabilityView {
            name: "PRISM Symbol Snapshot".to_string(),
            uri_template: SYMBOL_RESOURCE_TEMPLATE_URI.to_string(),
            mime_type: "application/json".to_string(),
            description: "Read an exact structured symbol snapshot.".to_string(),
            example_uri: resource_example_uri("symbol"),
        },
        ResourceTemplateCapabilityView {
            name: "PRISM Lineage".to_string(),
            uri_template: LINEAGE_RESOURCE_TEMPLATE_URI.to_string(),
            mime_type: "application/json".to_string(),
            description: "Read lineage history and current nodes for a lineage id.".to_string(),
            example_uri: resource_example_uri("lineage"),
        },
        ResourceTemplateCapabilityView {
            name: "PRISM Task Replay".to_string(),
            uri_template: TASK_RESOURCE_TEMPLATE_URI.to_string(),
            mime_type: "application/json".to_string(),
            description: "Read the outcome-event timeline for a task context.".to_string(),
            example_uri: resource_example_uri("task"),
        },
        ResourceTemplateCapabilityView {
            name: "PRISM Event".to_string(),
            uri_template: EVENT_RESOURCE_TEMPLATE_URI.to_string(),
            mime_type: "application/json".to_string(),
            description: "Read a single recorded outcome event by id.".to_string(),
            example_uri: resource_example_uri("event"),
        },
        ResourceTemplateCapabilityView {
            name: "PRISM Memory".to_string(),
            uri_template: MEMORY_RESOURCE_TEMPLATE_URI.to_string(),
            mime_type: "application/json".to_string(),
            description: "Read a single episodic memory entry by id.".to_string(),
            example_uri: resource_example_uri("memory"),
        },
        ResourceTemplateCapabilityView {
            name: "PRISM Inferred Edge".to_string(),
            uri_template: EDGE_RESOURCE_TEMPLATE_URI.to_string(),
            mime_type: "application/json".to_string(),
            description: "Read a single inferred-edge record by id.".to_string(),
            example_uri: resource_example_uri("edge"),
        },
    ]
}
