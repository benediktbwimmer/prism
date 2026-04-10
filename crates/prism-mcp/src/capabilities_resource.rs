use rmcp::model::ProtocolVersion;

use crate::{
    capabilities_resource_uri, capabilities_resource_view_link, capabilities_section_resource_uri,
    instructions_resource_view_link, resource_example_resource_uri, resource_example_uri,
    resource_link_view, resource_schema_catalog_entries, resource_shape_resource_uri,
    schema_resource_uri, schema_resource_view_link, search_resource_view_link_with_options,
    self_description_audit_resource_uri, session_resource_view_link,
    tool_action_example_resource_uri, tool_action_recipe_resource_uri,
    tool_action_schema_resource_uri, tool_action_shape_resource_uri, tool_example_resource_uri,
    tool_schema_catalog_entries, tool_schema_resource_uri, tool_schemas_resource_view_link,
    tool_shape_resource_uri, tool_variant_example_resource_uri, tool_variant_recipe_resource_uri,
    tool_variant_schema_resource_uri, tool_variant_shape_resource_uri, vocab_entry_resource_uri,
    workspace_revision_view, CapabilitiesBuildInfoView, CapabilitiesResourcePayload,
    FeatureFlagsView, PrismMcpFeatures, QueryHost, QueryMethodCapabilityView,
    ResourceCapabilityView, ResourceTemplateCapabilityView, RuntimeCapabilitiesView,
    ToolCapabilityView, API_REFERENCE_URI, CAPABILITIES_SECTION_RESOURCE_TEMPLATE_URI,
    CAPABILITIES_URI, CONTRACTS_RESOURCE_TEMPLATE_URI, CONTRACTS_URI, EDGE_RESOURCE_TEMPLATE_URI,
    ENTRYPOINTS_RESOURCE_TEMPLATE_URI, EVENT_RESOURCE_TEMPLATE_URI, FILE_RESOURCE_TEMPLATE_URI,
    LINEAGE_RESOURCE_TEMPLATE_URI, MEMORY_RESOURCE_TEMPLATE_URI, PLANS_RESOURCE_TEMPLATE_URI,
    PLANS_URI, PLAN_RESOURCE_TEMPLATE_URI, PROTECTED_STATE_URI,
    RESOURCE_EXAMPLE_RESOURCE_TEMPLATE_URI, RESOURCE_SHAPE_RESOURCE_TEMPLATE_URI, SCHEMAS_URI,
    SEARCH_RESOURCE_TEMPLATE_URI, SELF_DESCRIPTION_AUDIT_URI, SESSION_URI,
    SYMBOL_RESOURCE_TEMPLATE_URI, TASK_RESOURCE_TEMPLATE_URI,
    TOOL_ACTION_EXAMPLE_RESOURCE_TEMPLATE_URI, TOOL_ACTION_RECIPE_RESOURCE_TEMPLATE_URI,
    TOOL_ACTION_SCHEMA_RESOURCE_TEMPLATE_URI, TOOL_ACTION_SHAPE_RESOURCE_TEMPLATE_URI,
    TOOL_EXAMPLE_RESOURCE_TEMPLATE_URI, TOOL_SCHEMAS_URI, TOOL_SCHEMA_RESOURCE_TEMPLATE_URI,
    TOOL_SHAPE_RESOURCE_TEMPLATE_URI, TOOL_VARIANT_EXAMPLE_RESOURCE_TEMPLATE_URI,
    TOOL_VARIANT_RECIPE_RESOURCE_TEMPLATE_URI, TOOL_VARIANT_SCHEMA_RESOURCE_TEMPLATE_URI,
    TOOL_VARIANT_SHAPE_RESOURCE_TEMPLATE_URI, VOCAB_ENTRY_RESOURCE_TEMPLATE_URI, VOCAB_URI,
};

pub(crate) fn capabilities_resource_value(
    host: &QueryHost,
) -> anyhow::Result<CapabilitiesResourcePayload> {
    let prism = host.current_prism();
    let mut related_resources = vec![
        instructions_resource_view_link(),
        capabilities_resource_view_link(),
        session_resource_view_link(),
        crate::plans_resource_view_link(),
        crate::vocab_resource_view_link(),
        tool_schemas_resource_view_link(),
    ];
    if host.features.resource_kind_visible("protected-state") {
        related_resources.push(crate::protected_state_resource_view_link());
    }
    if host.features.cognition_layer_enabled() {
        related_resources.extend([
            schema_resource_view_link("capabilities"),
            schema_resource_view_link("vocab"),
            schema_resource_view_link("session"),
            schema_resource_view_link("schemas"),
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
                None,
                None,
                None,
            ),
        ]);
        related_resources.extend(
            resource_schema_catalog_entries()
                .into_iter()
                .take(4)
                .map(|entry| schema_resource_view_link(&entry.resource_kind)),
        );
    }
    Ok(CapabilitiesResourcePayload {
        uri: capabilities_resource_uri(),
        schema_uri: schema_resource_uri("capabilities"),
        build: CapabilitiesBuildInfoView {
            server_name: env!("CARGO_PKG_NAME").to_string(),
            server_version: env!("CARGO_PKG_VERSION").to_string(),
            protocol_version: ProtocolVersion::LATEST.as_str().to_string(),
            workspace_revision: workspace_revision_view(prism.workspace_revision()),
            api_reference_uri: if host.features.cognition_layer_enabled() {
                API_REFERENCE_URI.to_string()
            } else {
                String::new()
            },
        },
        features: FeatureFlagsView {
            mode: host.features.mode_label().to_string(),
            runtime: RuntimeCapabilitiesView {
                mode: host.features.runtime_mode_label().to_string(),
                coordination: host.features.coordination_layer_enabled(),
                knowledge_storage: host.features.knowledge_storage_layer_enabled(),
                cognition: host.features.cognition_layer_enabled(),
            },
            coordination: crate::CoordinationFeaturesView {
                workflow: host.features.coordination.workflow,
                claims: host.features.coordination.claims,
                artifacts: host.features.coordination.artifacts,
            },
            ui: host.features.ui,
            internal_developer: host.features.internal_developer,
        },
        query_methods: query_method_capabilities(&host.features),
        query_views: host.query_view_capabilities(),
        resources: resource_capabilities(&host.features),
        resource_templates: resource_template_capabilities(&host.features),
        tools: tool_schema_catalog_entries()
            .into_iter()
            .filter(|entry| host.features.is_tool_enabled(&entry.tool_name))
            .map(|entry| ToolCapabilityView {
                name: entry.tool_name,
                description: entry.description,
                schema_uri: entry.schema_uri,
                example_input: entry.example_input,
                example_uri: if host.features.tool_example_resources_visible() {
                    entry.example_uri
                } else {
                    None
                },
                shape_uri: if host.features.tool_example_resources_visible() {
                    entry.shape_uri
                } else {
                    None
                },
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

pub(crate) fn query_method_specs() -> Vec<(
    &'static str,
    &'static str,
    Option<&'static str>,
    &'static str,
)> {
    vec![
        (
            "from",
            "core",
            None,
            "Target a peer runtime by `runtime_id` for the next chained query call, for example `prism.from(\"runtime-abc\").runtime.status()`.",
        ),
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
            "validateToolInput",
            "core",
            None,
            "Validate a PRISM MCP tool payload, normalize tagged shorthand, and return actionable issues plus exact schema URIs.",
        ),
        (
            "contract",
            "core",
            None,
            "Resolve the strongest contract packet for a query or stable handle.",
        ),
        (
            "contracts",
            "core",
            None,
            "List contract packets with typed filters matching the contracts resource surface.",
        ),
        (
            "contractsFor",
            "core",
            None,
            "List contract packets that govern or consume a target.",
        ),
        (
            "specs",
            "core",
            None,
            "List native implementation specs discovered from the configured spec root.",
        ),
        (
            "spec",
            "core",
            None,
            "Read one native spec document with its current local status posture.",
        ),
        (
            "specSyncBrief",
            "core",
            None,
            "Read one sync-oriented spec brief with required checklist items, coverage, and linked coordination refs.",
        ),
        (
            "specCoverage",
            "core",
            None,
            "Read local checklist coverage records for one native spec.",
        ),
        (
            "specSyncProvenance",
            "core",
            None,
            "Read local sync provenance records linking a native spec to authoritative coordination objects.",
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
            "Read an exact workspace file slice by path and line range, locally or through `prism.from(\"runtime-id\")` when you need a peer runtime view.",
        ),
        (
            "fileAround",
            "core",
            None,
            "Read a bounded workspace file slice around one line, locally or through `prism.from(\"runtime-id\")` for peer-enriched context.",
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
            "Recall anchored session memory. In prism_code TypeScript, call `prism.memory.recall(...)`.",
        ),
        (
            "memoryOutcomes",
            "memory",
            None,
            "Query outcome history with filters. In prism_code TypeScript, call `prism.memory.outcomes(...)`.",
        ),
        (
            "memoryEvents",
            "memory",
            None,
            "Inspect raw memory event history with scope and provenance filters. In prism_code TypeScript, call `prism.memory.events(...)`.",
        ),
        (
            "curatorJobs",
            "curator",
            None,
            "Inspect recorded curator jobs.",
        ),
        (
            "curatorProposals",
            "curator",
            None,
            "Inspect curator proposals across jobs with flat filtering.",
        ),
        ("curatorJob", "curator", None, "Inspect one curator job."),
        (
            "plans",
            "coordination",
            Some("workflow"),
            "Discover plans with compact status, scope, and progress filters.",
        ),
        (
            "plan",
            "coordination",
            Some("workflow"),
            "Read a coordination plan.",
        ),
        (
            "planSummary",
            "coordination",
            Some("workflow"),
            "Summarize native plan progress, execution blockers, and completion gates.",
        ),
        ("task", "coordination", Some("workflow"), "Read one coordination task."),
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
            "taskEvidenceStatus",
            "coordination",
            Some("artifacts"),
            "Summarize coordination artifact, review, blocker, and validation posture for one task.",
        ),
        (
            "taskReviewStatus",
            "coordination",
            Some("artifacts"),
            "Summarize coordination review posture for one task.",
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
            "Inspect the MCP daemon status, health, process counts, and runtime file paths for this workspace. In `prism_code`, `prism.from(\"runtime-id\").runtime.status()` returns the peer-enriched equivalent for another runtime.",
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
            "mcpLog",
            "internal",
            Some("internal_developer"),
            "List recent durable MCP calls across tools, resources, and list operations.",
        ),
        (
            "slowMcpCalls",
            "internal",
            Some("internal_developer"),
            "List slow durable MCP calls with duration-based filtering and sorting.",
        ),
        (
            "mcpTrace",
            "internal",
            Some("internal_developer"),
            "Inspect the phase-by-phase trace and previews for one recorded MCP call.",
        ),
        (
            "mcpStats",
            "internal",
            Some("internal_developer"),
            "Aggregate durable MCP call counts and latency buckets by type and name.",
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
            "validationFeedback",
            "internal",
            Some("internal_developer"),
            "Read recent validation-feedback entries recorded while dogfooding PRISM on this workspace.",
        ),
        (
            "diagnostics",
            "core",
            None,
            "Return diagnostics gathered during the current query.",
        ),
    ]
}

pub(crate) fn resource_capabilities(features: &PrismMcpFeatures) -> Vec<ResourceCapabilityView> {
    let mut resources =
        crate::instructions::instruction_resource_capabilities(features.runtime_mode());
    if features.cognition_layer_enabled() {
        resources.push(ResourceCapabilityView {
            name: "PRISM API Reference".to_string(),
            uri: API_REFERENCE_URI.to_string(),
            mime_type: "text/markdown".to_string(),
            description: "TypeScript query surface, d.ts-style contract, and usage recipes."
                .to_string(),
            schema_uri: None,
            example_uri: None,
            shape_uri: None,
        });
    }
    resources.extend([
        ResourceCapabilityView {
            name: "PRISM Capabilities".to_string(),
            uri: CAPABILITIES_URI.to_string(),
            mime_type: "application/json".to_string(),
            description:
                "Canonical capability map for query methods, resources, features, and build info."
                    .to_string(),
            schema_uri: Some(schema_resource_uri("capabilities")),
            example_uri: if features.resource_example_resources_visible() {
                resource_example_uri("capabilities")
            } else {
                None
            },
            shape_uri: if features.resource_example_resources_visible() {
                Some(resource_shape_resource_uri("capabilities"))
            } else {
                None
            },
        },
        ResourceCapabilityView {
            name: "PRISM Session".to_string(),
            uri: SESSION_URI.to_string(),
            mime_type: "application/json".to_string(),
            description: "Active workspace root, task context, limits, and feature flags."
                .to_string(),
            schema_uri: Some(schema_resource_uri("session")),
            example_uri: if features.resource_example_resources_visible() {
                resource_example_uri("session")
            } else {
                None
            },
            shape_uri: if features.resource_example_resources_visible() {
                Some(resource_shape_resource_uri("session"))
            } else {
                None
            },
        },
        ResourceCapabilityView {
            name: "PRISM Vocabulary".to_string(),
            uri: VOCAB_URI.to_string(),
            mime_type: "application/json".to_string(),
            description:
                "Canonical enum and action vocabularies for PRISM MCP resources, query args, and mutation payloads."
                    .to_string(),
            schema_uri: Some(schema_resource_uri("vocab")),
            example_uri: if features.resource_example_resources_visible() {
                resource_example_uri("vocab")
            } else {
                None
            },
            shape_uri: if features.resource_example_resources_visible() {
                Some(resource_shape_resource_uri("vocab"))
            } else {
                None
            },
        },
        ResourceCapabilityView {
            name: "PRISM Plans".to_string(),
            uri: PLANS_URI.to_string(),
            mime_type: "application/json".to_string(),
            description:
                "Browse plans with compact progress summaries and optional coordination filters."
                    .to_string(),
            schema_uri: Some(schema_resource_uri("plans")),
            example_uri: if features.resource_example_resources_visible() {
                resource_example_uri("plans")
            } else {
                None
            },
            shape_uri: if features.resource_example_resources_visible() {
                Some(resource_shape_resource_uri("plans"))
            } else {
                None
            },
        },
        ResourceCapabilityView {
            name: "PRISM Tool Schemas".to_string(),
            uri: TOOL_SCHEMAS_URI.to_string(),
            mime_type: "application/json".to_string(),
            description: "Catalog of JSON Schemas for PRISM MCP tool inputs.".to_string(),
            schema_uri: Some(schema_resource_uri("tool-schemas")),
            example_uri: if features.resource_example_resources_visible() {
                resource_example_uri("tool-schemas")
            } else {
                None
            },
            shape_uri: if features.resource_example_resources_visible() {
                Some(resource_shape_resource_uri("tool-schemas"))
            } else {
                None
            },
        },
    ]);
    if features.resource_kind_visible("protected-state") {
        resources.push(ResourceCapabilityView {
            name: "PRISM Protected State".to_string(),
            uri: PROTECTED_STATE_URI.to_string(),
            mime_type: "application/json".to_string(),
            description:
                "Protected .prism stream verification status, trust diagnostics, and repair guidance."
                    .to_string(),
            schema_uri: Some(schema_resource_uri("protected-state")),
            example_uri: if features.resource_example_resources_visible() {
                resource_example_uri("protected-state")
            } else {
                None
            },
            shape_uri: if features.resource_example_resources_visible() {
                Some(resource_shape_resource_uri("protected-state"))
            } else {
                None
            },
        });
    }
    if features.cognition_layer_enabled() {
        resources.extend([
            ResourceCapabilityView {
                name: "PRISM Contracts".to_string(),
                uri: CONTRACTS_URI.to_string(),
                mime_type: "application/json".to_string(),
                description:
                    "Browse contract packets with compact status, scope, and promise metadata."
                        .to_string(),
                schema_uri: Some(schema_resource_uri("contracts")),
                example_uri: resource_example_uri("contracts"),
                shape_uri: Some(resource_shape_resource_uri("contracts")),
            },
            ResourceCapabilityView {
                name: "PRISM Resource Schemas".to_string(),
                uri: SCHEMAS_URI.to_string(),
                mime_type: "application/json".to_string(),
                description: "Catalog of JSON Schemas for structured PRISM resource payloads."
                    .to_string(),
                schema_uri: Some(schema_resource_uri("schemas")),
                example_uri: resource_example_uri("schemas"),
                shape_uri: Some(resource_shape_resource_uri("schemas")),
            },
            ResourceCapabilityView {
                name: "PRISM Self-Description Audit".to_string(),
                uri: SELF_DESCRIPTION_AUDIT_URI.to_string(),
                mime_type: "application/json".to_string(),
                description:
                    "Audit the MCP self-description surface, compact companions, and truncation-risk byte budgets."
                        .to_string(),
                schema_uri: Some(schema_resource_uri("self-description-audit")),
                example_uri: Some(self_description_audit_resource_uri()),
                shape_uri: Some(resource_shape_resource_uri("self-description-audit")),
            },
        ]);
    }
    resources
}

pub(crate) fn resource_template_capabilities(
    features: &PrismMcpFeatures,
) -> Vec<ResourceTemplateCapabilityView> {
    if features.runtime_mode() == prism_core::PrismRuntimeMode::CoordinationOnly {
        return vec![
            ResourceTemplateCapabilityView {
                name: "PRISM Plans Page".to_string(),
                uri_template: PLANS_RESOURCE_TEMPLATE_URI.to_string(),
                mime_type: "application/json".to_string(),
                description:
                    "Read plan discovery results with optional status, scope, text, sort, and pagination filters."
                        .to_string(),
                example_uri: Some(format!("{PLANS_URI}?limit=5")),
                shape_uri: None,
            },
            ResourceTemplateCapabilityView {
                name: "PRISM Plan".to_string(),
                uri_template: PLAN_RESOURCE_TEMPLATE_URI.to_string(),
                mime_type: "application/json".to_string(),
                description: "Read a coordination plan by id.".to_string(),
                example_uri: None,
                shape_uri: None,
            },
            ResourceTemplateCapabilityView {
                name: "PRISM Resource Schema".to_string(),
                uri_template: crate::SCHEMA_RESOURCE_TEMPLATE_URI.to_string(),
                mime_type: "application/schema+json".to_string(),
                description: "Read a JSON Schema for a structured PRISM resource kind.".to_string(),
                example_uri: Some(schema_resource_uri("capabilities")),
                shape_uri: None,
            },
            ResourceTemplateCapabilityView {
                name: "PRISM Tool Schema".to_string(),
                uri_template: TOOL_SCHEMA_RESOURCE_TEMPLATE_URI.to_string(),
                mime_type: "application/schema+json".to_string(),
                description: "Read a JSON Schema for a PRISM MCP tool input payload.".to_string(),
                example_uri: Some(tool_schema_resource_uri("prism_code")),
                shape_uri: None,
            },
            ResourceTemplateCapabilityView {
                name: "PRISM Tool Action Schema".to_string(),
                uri_template: TOOL_ACTION_SCHEMA_RESOURCE_TEMPLATE_URI.to_string(),
                mime_type: "application/schema+json".to_string(),
                description: "Read an exact JSON Schema for one tagged PRISM MCP tool action."
                    .to_string(),
                example_uri: Some(tool_action_schema_resource_uri(
                    "prism_mutate",
                    "coordination",
                )),
                shape_uri: None,
            },
            ResourceTemplateCapabilityView {
                name: "PRISM Tool Variant Schema".to_string(),
                uri_template: TOOL_VARIANT_SCHEMA_RESOURCE_TEMPLATE_URI.to_string(),
                mime_type: "application/schema+json".to_string(),
                description:
                    "Read an exact JSON Schema for one nested tool payload variant.".to_string(),
                example_uri: Some(tool_variant_schema_resource_uri(
                    "prism_mutate",
                    "coordination",
                    "plan_bootstrap",
                )),
                shape_uri: None,
            },
            ResourceTemplateCapabilityView {
                name: "PRISM Capabilities Section".to_string(),
                uri_template: CAPABILITIES_SECTION_RESOURCE_TEMPLATE_URI.to_string(),
                mime_type: "application/json".to_string(),
                description: "Read one segmented section of the capabilities resource."
                    .to_string(),
                example_uri: Some(capabilities_section_resource_uri("tools")),
                shape_uri: None,
            },
            ResourceTemplateCapabilityView {
                name: "PRISM Vocabulary Entry".to_string(),
                uri_template: VOCAB_ENTRY_RESOURCE_TEMPLATE_URI.to_string(),
                mime_type: "application/json".to_string(),
                description: "Read one segmented vocabulary entry by key.".to_string(),
                example_uri: Some(vocab_entry_resource_uri("coordinationMutationKind")),
                shape_uri: None,
            },
        ];
    }
    let example_resource_kind = if features.resource_kind_visible("search") {
        "search"
    } else {
        "plan"
    };
    let example_tool_name = if features.is_tool_enabled("prism_code") {
        "prism_code"
    } else if features.is_tool_enabled("prism_query") {
        "prism_query"
    } else {
        "prism_mutate"
    };
    let mut templates = vec![
        ResourceTemplateCapabilityView {
            name: "PRISM Plans Page".to_string(),
            uri_template: PLANS_RESOURCE_TEMPLATE_URI.to_string(),
            mime_type: "application/json".to_string(),
            description:
                "Read plan discovery results with optional status, scope, text, sort, and pagination filters."
                    .to_string(),
            example_uri: resource_example_uri("plans"),
            shape_uri: Some(resource_shape_resource_uri("plans")),
        },
        ResourceTemplateCapabilityView {
            name: "PRISM Plan".to_string(),
            uri_template: PLAN_RESOURCE_TEMPLATE_URI.to_string(),
            mime_type: "application/json".to_string(),
            description: "Read a coordination plan by id.".to_string(),
            example_uri: resource_example_uri("plan"),
            shape_uri: Some(resource_shape_resource_uri("plan")),
        },
        ResourceTemplateCapabilityView {
            name: "PRISM Resource Schema".to_string(),
            uri_template: crate::SCHEMA_RESOURCE_TEMPLATE_URI.to_string(),
            mime_type: "application/schema+json".to_string(),
            description: "Read a JSON Schema for a structured PRISM resource kind.".to_string(),
            example_uri: Some(schema_resource_uri(example_resource_kind)),
            shape_uri: Some(resource_shape_resource_uri(example_resource_kind)),
        },
        ResourceTemplateCapabilityView {
            name: "PRISM Tool Schema".to_string(),
            uri_template: TOOL_SCHEMA_RESOURCE_TEMPLATE_URI.to_string(),
            mime_type: "application/schema+json".to_string(),
            description: "Read a JSON Schema for a PRISM MCP tool input payload.".to_string(),
            example_uri: Some(tool_schema_resource_uri(example_tool_name)),
            shape_uri: if features.tool_example_resources_visible() {
                Some(tool_shape_resource_uri(example_tool_name))
            } else {
                None
            },
        },
        ResourceTemplateCapabilityView {
            name: "PRISM Tool Action Schema".to_string(),
            uri_template: TOOL_ACTION_SCHEMA_RESOURCE_TEMPLATE_URI.to_string(),
            mime_type: "application/schema+json".to_string(),
            description: "Read an exact JSON Schema for one tagged PRISM MCP tool action.".to_string(),
            example_uri: Some(tool_action_schema_resource_uri(
                "prism_mutate",
                if features.tool_example_resources_visible() {
                    "validation_feedback"
                } else {
                    "coordination"
                },
            )),
            shape_uri: if features.tool_example_resources_visible() {
                Some(tool_action_shape_resource_uri(
                    "prism_mutate",
                    "validation_feedback",
                ))
            } else {
                None
            },
        },
        ResourceTemplateCapabilityView {
            name: "PRISM Tool Variant Schema".to_string(),
            uri_template: TOOL_VARIANT_SCHEMA_RESOURCE_TEMPLATE_URI.to_string(),
            mime_type: "application/schema+json".to_string(),
            description: "Read an exact JSON Schema for one nested tool payload variant.".to_string(),
            example_uri: Some(tool_variant_schema_resource_uri(
                "prism_mutate",
                "coordination",
                "plan_bootstrap",
            )),
            shape_uri: if features.tool_example_resources_visible() {
                Some(tool_variant_shape_resource_uri(
                    "prism_mutate",
                    "coordination",
                    "plan_bootstrap",
                ))
            } else {
                None
            },
        },
        ResourceTemplateCapabilityView {
            name: "PRISM Tool Example".to_string(),
            uri_template: TOOL_EXAMPLE_RESOURCE_TEMPLATE_URI.to_string(),
            mime_type: "application/json".to_string(),
            description: "Read compact example payloads for one PRISM MCP tool.".to_string(),
            example_uri: Some(tool_example_resource_uri("prism_mutate")),
            shape_uri: Some(resource_shape_resource_uri("tool-example")),
        },
        ResourceTemplateCapabilityView {
            name: "PRISM Tool Action Example".to_string(),
            uri_template: TOOL_ACTION_EXAMPLE_RESOURCE_TEMPLATE_URI.to_string(),
            mime_type: "application/json".to_string(),
            description: "Read compact example payloads for one tagged PRISM MCP tool action.".to_string(),
            example_uri: Some(tool_action_example_resource_uri(
                "prism_mutate",
                "coordination",
            )),
            shape_uri: Some(resource_shape_resource_uri("tool-example")),
        },
        ResourceTemplateCapabilityView {
            name: "PRISM Tool Variant Example".to_string(),
            uri_template: TOOL_VARIANT_EXAMPLE_RESOURCE_TEMPLATE_URI.to_string(),
            mime_type: "application/json".to_string(),
            description: "Read compact example payloads for one nested tool payload variant.".to_string(),
            example_uri: Some(tool_variant_example_resource_uri(
                "prism_mutate",
                "coordination",
                "plan_bootstrap",
            )),
            shape_uri: Some(resource_shape_resource_uri("tool-example")),
        },
        ResourceTemplateCapabilityView {
            name: "PRISM Tool Shape".to_string(),
            uri_template: TOOL_SHAPE_RESOURCE_TEMPLATE_URI.to_string(),
            mime_type: "application/json".to_string(),
            description: "Read a compact shape summary for one PRISM MCP tool.".to_string(),
            example_uri: Some(tool_shape_resource_uri("prism_mutate")),
            shape_uri: Some(resource_shape_resource_uri("tool-shape")),
        },
        ResourceTemplateCapabilityView {
            name: "PRISM Tool Action Shape".to_string(),
            uri_template: TOOL_ACTION_SHAPE_RESOURCE_TEMPLATE_URI.to_string(),
            mime_type: "application/json".to_string(),
            description: "Read a compact shape summary for one tagged PRISM MCP tool action.".to_string(),
            example_uri: Some(tool_action_shape_resource_uri(
                "prism_mutate",
                "coordination",
            )),
            shape_uri: Some(resource_shape_resource_uri("tool-shape")),
        },
        ResourceTemplateCapabilityView {
            name: "PRISM Tool Variant Shape".to_string(),
            uri_template: TOOL_VARIANT_SHAPE_RESOURCE_TEMPLATE_URI.to_string(),
            mime_type: "application/json".to_string(),
            description: "Read a compact shape summary for one nested tool payload variant.".to_string(),
            example_uri: Some(tool_variant_shape_resource_uri(
                "prism_mutate",
                "coordination",
                "plan_bootstrap",
            )),
            shape_uri: Some(resource_shape_resource_uri("tool-shape")),
        },
        ResourceTemplateCapabilityView {
            name: "PRISM Resource Example".to_string(),
            uri_template: RESOURCE_EXAMPLE_RESOURCE_TEMPLATE_URI.to_string(),
            mime_type: "application/json".to_string(),
            description: "Read a compact example payload for one structured PRISM resource kind.".to_string(),
            example_uri: Some(resource_example_resource_uri(example_resource_kind)),
            shape_uri: Some(resource_shape_resource_uri("resource-example")),
        },
        ResourceTemplateCapabilityView {
            name: "PRISM Resource Shape".to_string(),
            uri_template: RESOURCE_SHAPE_RESOURCE_TEMPLATE_URI.to_string(),
            mime_type: "application/json".to_string(),
            description: "Read a compact shape summary for one structured PRISM resource kind.".to_string(),
            example_uri: Some(resource_shape_resource_uri(example_resource_kind)),
            shape_uri: Some(resource_shape_resource_uri("resource-shape")),
        },
        ResourceTemplateCapabilityView {
            name: "PRISM Capabilities Section".to_string(),
            uri_template: CAPABILITIES_SECTION_RESOURCE_TEMPLATE_URI.to_string(),
            mime_type: "application/json".to_string(),
            description: "Read one segmented section of the capabilities resource.".to_string(),
            example_uri: Some(capabilities_section_resource_uri("tools")),
            shape_uri: Some(resource_shape_resource_uri("capabilities-section")),
        },
        ResourceTemplateCapabilityView {
            name: "PRISM Vocabulary Entry".to_string(),
            uri_template: VOCAB_ENTRY_RESOURCE_TEMPLATE_URI.to_string(),
            mime_type: "application/json".to_string(),
            description: "Read one segmented vocabulary entry by key.".to_string(),
            example_uri: Some(vocab_entry_resource_uri("coordinationMutationKind")),
            shape_uri: Some(resource_shape_resource_uri("vocab-entry")),
        },
        ResourceTemplateCapabilityView {
            name: "PRISM Tool Action Recipe".to_string(),
            uri_template: TOOL_ACTION_RECIPE_RESOURCE_TEMPLATE_URI.to_string(),
            mime_type: "text/markdown".to_string(),
            description: "Read a short operator recipe for one tagged tool action.".to_string(),
            example_uri: Some(tool_action_recipe_resource_uri(
                "prism_mutate",
                "validation_feedback",
            )),
            shape_uri: None,
        },
        ResourceTemplateCapabilityView {
            name: "PRISM Tool Variant Recipe".to_string(),
            uri_template: TOOL_VARIANT_RECIPE_RESOURCE_TEMPLATE_URI.to_string(),
            mime_type: "text/markdown".to_string(),
            description: "Read a short operator recipe for one nested tool payload variant.".to_string(),
            example_uri: Some(tool_variant_recipe_resource_uri(
                "prism_mutate",
                "coordination",
                "plan_bootstrap",
            )),
            shape_uri: None,
        },
    ];
    if features.cognition_layer_enabled() {
        templates.splice(
            0..0,
            [ResourceTemplateCapabilityView {
                name: "PRISM Entrypoints Page".to_string(),
                uri_template: ENTRYPOINTS_RESOURCE_TEMPLATE_URI.to_string(),
                mime_type: "application/json".to_string(),
                description: "Read workspace entrypoints with optional pagination.".to_string(),
                example_uri: resource_example_uri("entrypoints"),
                shape_uri: Some(resource_shape_resource_uri("entrypoints")),
            }],
        );
        templates.insert(
            2,
            ResourceTemplateCapabilityView {
                name: "PRISM Contracts Page".to_string(),
                uri_template: CONTRACTS_RESOURCE_TEMPLATE_URI.to_string(),
                mime_type: "application/json".to_string(),
                description:
                    "Read contract discovery results with optional text, status, scope, and kind filters."
                        .to_string(),
                example_uri: resource_example_uri("contracts"),
                shape_uri: Some(resource_shape_resource_uri("contracts")),
            },
        );
        templates.extend([
            ResourceTemplateCapabilityView {
                name: "PRISM Search".to_string(),
                uri_template: SEARCH_RESOURCE_TEMPLATE_URI.to_string(),
                mime_type: "application/json".to_string(),
                description: "Read structured search results and diagnostics for a query."
                    .to_string(),
                example_uri: resource_example_uri("search"),
                shape_uri: Some(resource_shape_resource_uri("search")),
            },
            ResourceTemplateCapabilityView {
                name: "PRISM File".to_string(),
                uri_template: FILE_RESOURCE_TEMPLATE_URI.to_string(),
                mime_type: "application/json".to_string(),
                description:
                    "Read a workspace file excerpt by path with optional line-range narrowing."
                        .to_string(),
                example_uri: resource_example_uri("file"),
                shape_uri: Some(resource_shape_resource_uri("file")),
            },
            ResourceTemplateCapabilityView {
                name: "PRISM Symbol Snapshot".to_string(),
                uri_template: SYMBOL_RESOURCE_TEMPLATE_URI.to_string(),
                mime_type: "application/json".to_string(),
                description: "Read an exact structured symbol snapshot.".to_string(),
                example_uri: resource_example_uri("symbol"),
                shape_uri: Some(resource_shape_resource_uri("symbol")),
            },
            ResourceTemplateCapabilityView {
                name: "PRISM Lineage".to_string(),
                uri_template: LINEAGE_RESOURCE_TEMPLATE_URI.to_string(),
                mime_type: "application/json".to_string(),
                description: "Read lineage history and current nodes for a lineage id.".to_string(),
                example_uri: resource_example_uri("lineage"),
                shape_uri: Some(resource_shape_resource_uri("lineage")),
            },
            ResourceTemplateCapabilityView {
                name: "PRISM Task Replay".to_string(),
                uri_template: TASK_RESOURCE_TEMPLATE_URI.to_string(),
                mime_type: "application/json".to_string(),
                description: "Read the outcome-event timeline for a task context.".to_string(),
                example_uri: resource_example_uri("task"),
                shape_uri: Some(resource_shape_resource_uri("task")),
            },
            ResourceTemplateCapabilityView {
                name: "PRISM Event".to_string(),
                uri_template: EVENT_RESOURCE_TEMPLATE_URI.to_string(),
                mime_type: "application/json".to_string(),
                description: "Read a single recorded outcome event by id.".to_string(),
                example_uri: resource_example_uri("event"),
                shape_uri: Some(resource_shape_resource_uri("event")),
            },
            ResourceTemplateCapabilityView {
                name: "PRISM Memory".to_string(),
                uri_template: MEMORY_RESOURCE_TEMPLATE_URI.to_string(),
                mime_type: "application/json".to_string(),
                description: "Read a single episodic memory entry by id.".to_string(),
                example_uri: resource_example_uri("memory"),
                shape_uri: Some(resource_shape_resource_uri("memory")),
            },
            ResourceTemplateCapabilityView {
                name: "PRISM Inferred Edge".to_string(),
                uri_template: EDGE_RESOURCE_TEMPLATE_URI.to_string(),
                mime_type: "application/json".to_string(),
                description: "Read a single inferred-edge record by id.".to_string(),
                example_uri: resource_example_uri("edge"),
                shape_uri: Some(resource_shape_resource_uri("edge")),
            },
        ]);
    }
    if !features.tool_example_resources_visible() || !features.resource_example_resources_visible()
    {
        templates.retain(|template| {
            !matches!(
                template.uri_template.as_str(),
                TOOL_EXAMPLE_RESOURCE_TEMPLATE_URI
                    | TOOL_ACTION_EXAMPLE_RESOURCE_TEMPLATE_URI
                    | TOOL_VARIANT_EXAMPLE_RESOURCE_TEMPLATE_URI
                    | TOOL_SHAPE_RESOURCE_TEMPLATE_URI
                    | TOOL_ACTION_SHAPE_RESOURCE_TEMPLATE_URI
                    | TOOL_VARIANT_SHAPE_RESOURCE_TEMPLATE_URI
                    | TOOL_ACTION_RECIPE_RESOURCE_TEMPLATE_URI
                    | TOOL_VARIANT_RECIPE_RESOURCE_TEMPLATE_URI
                    | RESOURCE_EXAMPLE_RESOURCE_TEMPLATE_URI
                    | RESOURCE_SHAPE_RESOURCE_TEMPLATE_URI
            )
        });
    }
    templates
}
