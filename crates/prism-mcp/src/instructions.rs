use prism_core::PrismRuntimeMode;
use rmcp::model::RawResource;

use crate::{
    CAPABILITIES_URI, INSTRUCTIONS_URI, PLANS_URI, PROTECTED_STATE_URI, PrismMcpFeatures,
    ResourceLinkView, SESSION_URI, TOOL_SCHEMAS_URI, VOCAB_URI, resource_link_view,
    resource_schemas::ResourceCapabilityView,
};

const INDEX_PLACEHOLDER: &str = "{{INSTRUCTION_SET_INDEX}}";
const SHARED_BLOCKS_PLACEHOLDER: &str = "{{SHARED_BLOCKS}}";

const INDEX_MARKDOWN: &str = include_str!("../../../docs/prism/instructions/index.md");
const EXECUTION_MARKDOWN: &str = include_str!("../../../docs/prism/instructions/execution.md");
const PLANNING_MARKDOWN: &str = include_str!("../../../docs/prism/instructions/planning.md");
const REVIEW_MARKDOWN: &str = include_str!("../../../docs/prism/instructions/review.md");
const COORDINATION_MARKDOWN: &str =
    include_str!("../../../docs/prism/instructions/coordination.md");
const EXPLORATION_MARKDOWN: &str = include_str!("../../../docs/prism/instructions/exploration.md");

const FAMILIARIZATION_BLOCK: &str =
    include_str!("../../../docs/prism/instructions/blocks/familiarization.md");
const DEFAULT_PATH_BLOCK: &str =
    include_str!("../../../docs/prism/instructions/blocks/default-path.md");
const DEFAULT_PATH_COORDINATION_ONLY_BLOCK: &str =
    include_str!("../../../docs/prism/instructions/blocks/default-path-coordination-only.md");
const QUERY_VIEWS_BLOCK: &str =
    include_str!("../../../docs/prism/instructions/blocks/query-views.md");
const READ_STRATEGY_BLOCK: &str =
    include_str!("../../../docs/prism/instructions/blocks/read-strategy.md");
const COMPRESSION_BLOCK: &str =
    include_str!("../../../docs/prism/instructions/blocks/compression.md");
const PLANS_BLOCK: &str = include_str!("../../../docs/prism/instructions/blocks/plans.md");
const MUTATIONS_BLOCK: &str = include_str!("../../../docs/prism/instructions/blocks/mutations.md");
const MEMORY_GUIDANCE_BLOCK: &str =
    include_str!("../../../docs/prism/instructions/blocks/memory-guidance.md");
const CONCEPT_PACKS_BLOCK: &str =
    include_str!("../../../docs/prism/instructions/blocks/concept-packs.md");

#[derive(Clone, Copy)]
pub(crate) struct InstructionSetDefinition {
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    pub(crate) description: &'static str,
    pub(crate) use_when: &'static str,
    markdown: &'static str,
    block_markdowns: &'static [&'static str],
}

const EXECUTION_BLOCKS: &[&str] = &[
    FAMILIARIZATION_BLOCK,
    DEFAULT_PATH_BLOCK,
    QUERY_VIEWS_BLOCK,
    READ_STRATEGY_BLOCK,
    COMPRESSION_BLOCK,
    PLANS_BLOCK,
    MUTATIONS_BLOCK,
    MEMORY_GUIDANCE_BLOCK,
    CONCEPT_PACKS_BLOCK,
];

const PLANNING_BLOCKS: &[&str] = &[
    FAMILIARIZATION_BLOCK,
    DEFAULT_PATH_BLOCK,
    QUERY_VIEWS_BLOCK,
    READ_STRATEGY_BLOCK,
    COMPRESSION_BLOCK,
    PLANS_BLOCK,
    MUTATIONS_BLOCK,
    MEMORY_GUIDANCE_BLOCK,
    CONCEPT_PACKS_BLOCK,
];

const REVIEW_BLOCKS: &[&str] = &[
    FAMILIARIZATION_BLOCK,
    DEFAULT_PATH_BLOCK,
    QUERY_VIEWS_BLOCK,
    READ_STRATEGY_BLOCK,
    COMPRESSION_BLOCK,
    MUTATIONS_BLOCK,
    MEMORY_GUIDANCE_BLOCK,
    CONCEPT_PACKS_BLOCK,
];

const COORDINATION_BLOCKS: &[&str] = &[
    FAMILIARIZATION_BLOCK,
    DEFAULT_PATH_BLOCK,
    QUERY_VIEWS_BLOCK,
    READ_STRATEGY_BLOCK,
    COMPRESSION_BLOCK,
    PLANS_BLOCK,
    MUTATIONS_BLOCK,
    MEMORY_GUIDANCE_BLOCK,
    CONCEPT_PACKS_BLOCK,
];

const COORDINATION_ONLY_BLOCKS: &[&str] = &[
    FAMILIARIZATION_BLOCK,
    DEFAULT_PATH_COORDINATION_ONLY_BLOCK,
    PLANS_BLOCK,
    MUTATIONS_BLOCK,
];

const EXPLORATION_BLOCKS: &[&str] = &[
    FAMILIARIZATION_BLOCK,
    DEFAULT_PATH_BLOCK,
    QUERY_VIEWS_BLOCK,
    READ_STRATEGY_BLOCK,
    COMPRESSION_BLOCK,
    MUTATIONS_BLOCK,
    MEMORY_GUIDANCE_BLOCK,
    CONCEPT_PACKS_BLOCK,
];

const INSTRUCTION_SET_DEFINITIONS: &[InstructionSetDefinition] = &[
    InstructionSetDefinition {
        id: "execution",
        name: "PRISM Instructions: Execution",
        description: "Task execution guidance for actionable nodes, implementation, validation, and completion.",
        use_when: "Load when the prompt is about starting actionable task nodes, implementing concrete changes, or continuing claimed execution work.",
        markdown: EXECUTION_MARKDOWN,
        block_markdowns: EXECUTION_BLOCKS,
    },
    InstructionSetDefinition {
        id: "planning",
        name: "PRISM Instructions: Planning",
        description: "Plan authoring guidance for decomposition, dependency shaping, and priority decisions.",
        use_when: "Load when the prompt is about creating, refining, or restructuring a PRISM plan.",
        markdown: PLANNING_MARKDOWN,
        block_markdowns: PLANNING_BLOCKS,
    },
    InstructionSetDefinition {
        id: "review",
        name: "PRISM Instructions: Review",
        description: "Review and validation guidance for findings, regressions, and readiness checks.",
        use_when: "Load when the prompt is about reviewing work, validating behavior, or identifying regressions and risks.",
        markdown: REVIEW_MARKDOWN,
        block_markdowns: REVIEW_BLOCKS,
    },
    InstructionSetDefinition {
        id: "coordination",
        name: "PRISM Instructions: Coordination",
        description: "Coordination guidance for claims, handoffs, readiness, and multi-agent execution flow.",
        use_when: "Load when the prompt is about task availability, shared claims, handoffs, or repo-wide execution coordination.",
        markdown: COORDINATION_MARKDOWN,
        block_markdowns: COORDINATION_BLOCKS,
    },
    InstructionSetDefinition {
        id: "exploration",
        name: "PRISM Instructions: Exploration",
        description: "Exploration guidance for repo understanding, owner discovery, and bounded semantic orientation.",
        use_when: "Load when the prompt is about understanding an unfamiliar subsystem or building context before planning or execution.",
        markdown: EXPLORATION_MARKDOWN,
        block_markdowns: EXPLORATION_BLOCKS,
    },
];

pub(crate) fn instructions_resource_uri() -> String {
    INSTRUCTIONS_URI.to_string()
}

pub(crate) fn instruction_set_resource_uri(id: &str) -> String {
    format!("{INSTRUCTIONS_URI}/{id}")
}

pub(crate) fn instructions_resource_link() -> RawResource {
    RawResource::new(instructions_resource_uri(), "PRISM Instruction Sets")
        .with_description("Overview of the available PRISM role-specific instruction resources")
        .with_mime_type("text/markdown")
}

fn available_instruction_set_definitions(
    runtime_mode: PrismRuntimeMode,
) -> Vec<InstructionSetDefinition> {
    let definitions = instruction_set_definitions();
    if runtime_mode == PrismRuntimeMode::CoordinationOnly {
        return definitions
            .iter()
            .copied()
            .filter(|definition| definition.id == "coordination")
            .map(|mut definition| {
                definition.block_markdowns = COORDINATION_ONLY_BLOCKS;
                definition
            })
            .collect();
    }
    definitions.to_vec()
}

fn default_features_for_runtime_mode(runtime_mode: PrismRuntimeMode) -> PrismMcpFeatures {
    PrismMcpFeatures::full().with_runtime_mode(runtime_mode)
}

pub(crate) fn instruction_set_resource_links(runtime_mode: PrismRuntimeMode) -> Vec<RawResource> {
    available_instruction_set_definitions(runtime_mode)
        .iter()
        .map(|definition| {
            RawResource::new(instruction_set_resource_uri(definition.id), definition.name)
                .with_description(definition.description)
                .with_mime_type("text/markdown")
        })
        .collect()
}

pub(crate) fn instructions_resource_view_link() -> ResourceLinkView {
    resource_link_view(
        instructions_resource_uri(),
        "PRISM Instruction Sets",
        "Overview of the available PRISM role-specific instruction resources",
    )
}

pub(crate) fn instruction_resource_capabilities(
    runtime_mode: PrismRuntimeMode,
) -> Vec<ResourceCapabilityView> {
    let mut resources = vec![ResourceCapabilityView {
        name: "PRISM Instruction Sets".to_string(),
        uri: instructions_resource_uri(),
        mime_type: "text/markdown".to_string(),
        description: "Overview of the available PRISM role-specific instruction resources."
            .to_string(),
        schema_uri: None,
        example_uri: Some(instructions_resource_uri()),
        shape_uri: None,
    }];
    resources.extend(
        available_instruction_set_definitions(runtime_mode)
            .iter()
            .map(|definition| ResourceCapabilityView {
                name: definition.name.to_string(),
                uri: instruction_set_resource_uri(definition.id),
                mime_type: "text/markdown".to_string(),
                description: definition.description.to_string(),
                schema_uri: None,
                example_uri: Some(instruction_set_resource_uri(definition.id)),
                shape_uri: None,
            }),
    );
    resources
}

pub(crate) fn instruction_set_definitions() -> &'static [InstructionSetDefinition] {
    INSTRUCTION_SET_DEFINITIONS
}

pub(crate) fn parse_instruction_resource_uri(uri: &str) -> Option<Option<String>> {
    let base = uri.split_once('?').map(|(base, _)| base).unwrap_or(uri);
    if base == INSTRUCTIONS_URI {
        return Some(None);
    }
    let id = base.strip_prefix("prism://instructions/")?;
    if instruction_set_definition(id).is_some() {
        Some(Some(id.to_string()))
    } else {
        None
    }
}

pub(crate) fn render_instructions_index(runtime_mode: PrismRuntimeMode) -> String {
    render_instructions_index_with_features(&default_features_for_runtime_mode(runtime_mode))
}

pub(crate) fn render_instructions_index_with_features(features: &PrismMcpFeatures) -> String {
    if features.runtime_mode() == PrismRuntimeMode::CoordinationOnly {
        return coordination_only_index_markdown(features);
    }
    let runtime_mode = features.runtime_mode();
    let catalog = available_instruction_set_definitions(runtime_mode)
        .iter()
        .map(|definition| {
            format!(
                "- `{id}`: {description}\n  Read: `{uri}`\n  Use when: {use_when}",
                id = definition.id,
                description = definition.description,
                uri = instruction_set_resource_uri(definition.id),
                use_when = definition.use_when,
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let rendered = INDEX_MARKDOWN.replace(INDEX_PLACEHOLDER, &catalog);
    append_coordination_mode_note(rendered, features)
}

pub(crate) fn render_instruction_set(id: &str, runtime_mode: PrismRuntimeMode) -> Option<String> {
    render_instruction_set_with_features(id, &default_features_for_runtime_mode(runtime_mode))
}

pub(crate) fn render_instruction_set_with_features(
    id: &str,
    features: &PrismMcpFeatures,
) -> Option<String> {
    let runtime_mode = features.runtime_mode();
    let definition = available_instruction_set_definitions(runtime_mode)
        .into_iter()
        .find(|definition| definition.id == id)?;
    let blocks = definition
        .block_markdowns
        .iter()
        .map(|markdown| markdown.trim_end())
        .collect::<Vec<_>>()
        .join("\n\n");
    let rendered = definition
        .markdown
        .replace(SHARED_BLOCKS_PLACEHOLDER, &blocks);
    Some(append_coordination_mode_note(rendered, features))
}

fn instruction_set_definition(id: &str) -> Option<&'static InstructionSetDefinition> {
    instruction_set_definitions()
        .iter()
        .find(|definition| definition.id == id)
}

fn append_coordination_mode_note(mut markdown: String, features: &PrismMcpFeatures) -> String {
    let feature_summary = features.coordination_summary_lines().join("\n");
    if features.runtime_mode() == PrismRuntimeMode::CoordinationOnly {
        markdown.push_str(
            "\n\nThis instruction set is running without cognition. Use `prism_code` for the reduced coordination and operator read/write surface, use the native `prism` SDK methods for authoritative changes, and avoid graph-backed repo understanding or enrichment flows.",
        );
        markdown.push_str("\n\nMode contract:\n");
        markdown.push_str(&feature_summary);
        return markdown;
    }
    if features.runtime_mode() != PrismRuntimeMode::Full {
        markdown.push_str(
            "\n\nCoordination features are gated on this server; check `prism://session` before using plan, claim, or artifact workflows.",
        );
        markdown.push_str("\n\nMode contract:\n");
        markdown.push_str(&feature_summary);
    }
    markdown
}

fn coordination_only_index_markdown(features: &PrismMcpFeatures) -> String {
    let tools = ["prism_code", "prism_task_brief"]
        .into_iter()
        .filter(|tool| features.is_tool_enabled(tool))
        .map(|tool| format!("`{tool}`"))
        .collect::<Vec<_>>()
        .join(", ");
    let mut query_scope = vec![
        "coordination reads".to_string(),
        "tool/schema inspection".to_string(),
    ];
    if features.internal_developer_enabled() {
        query_scope.push("runtime and MCP diagnostics".to_string());
    }
    let resources = [
        CAPABILITIES_URI,
        SESSION_URI,
        PROTECTED_STATE_URI,
        VOCAB_URI,
        PLANS_URI,
        TOOL_SCHEMAS_URI,
    ]
    .into_iter()
    .filter(|uri| coordination_only_resource_uri_visible(features, uri))
    .map(|uri| format!("`{uri}`"))
    .collect::<Vec<_>>()
    .join(", ");
    format!(
        "# PRISM Coordination-Only Instructions\n\nThis server is running in `coordination_only` mode.\n\nMode contract:\n{}\n\nAvailable instruction set:\n- `coordination`: coordination workflows only\n  Read: `{}`\n\nAvailable public APIs in this mode:\n- tools: {}\n- query scope: {}\n- resources: {}\n\nUnavailable in this mode:\n- graph-backed repo exploration\n- concept and contract enrichment\n- dynamic query views\n- programmable graph query surfaces outside the reduced coordination and ops set",
        features.coordination_summary_lines().join("\n"),
        instruction_set_resource_uri("coordination"),
        tools,
        query_scope.join(", "),
        resources,
    )
}

fn coordination_only_resource_uri_visible(features: &PrismMcpFeatures, uri: &str) -> bool {
    match uri {
        CAPABILITIES_URI => features.resource_kind_visible("capabilities"),
        SESSION_URI => features.resource_kind_visible("session"),
        PROTECTED_STATE_URI => features.resource_kind_visible("protected-state"),
        VOCAB_URI => features.resource_kind_visible("vocab"),
        PLANS_URI => features.resource_kind_visible("plans"),
        TOOL_SCHEMAS_URI => features.resource_kind_visible("tool-schemas"),
        _ => false,
    }
}
