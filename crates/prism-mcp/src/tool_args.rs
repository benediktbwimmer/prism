use prism_js::{ConceptPacketView, ConceptRelationView, TaskJournalView};
use rmcp::schemars::{JsonSchema, Schema};
use serde::{de, Deserialize, Deserializer};
use serde_json::Value;

use crate::{tool_schema_view, vocabulary_error, SessionView};

fn ensure_root_object_input_schema(schema: &mut Schema) {
    if schema.get("type").is_none() {
        schema.insert("type".to_string(), Value::String("object".to_string()));
    }
}

fn parse_tagged_tool_input<T>(tool_name: &str, value: Value) -> Result<T, String>
where
    T: serde::de::DeserializeOwned,
{
    let used_flat_shorthand = is_flat_tagged_tool_input(&value);
    let value = normalize_tagged_tool_input(value);
    let tool = tool_schema_view(tool_name);
    let action = value
        .get("action")
        .and_then(Value::as_str)
        .map(ToString::to_string);

    if let Some(tool) = &tool {
        if !tool.actions.is_empty() {
            let valid_actions = tool
                .actions
                .iter()
                .map(|action| action.action.as_str())
                .collect::<Vec<_>>();
            match action.as_deref() {
                None => {
                    return Err(format!(
                        "{tool_name} requires `action`; valid actions: {}. Inspect via prism.tool(\"{tool_name}\").",
                        valid_actions.join(", ")
                    ));
                }
                Some(action_name) if !valid_actions.contains(&action_name) => {
                    return Err(format!(
                        "unknown {tool_name} action `{action_name}`; valid actions: {}. Inspect via prism.tool(\"{tool_name}\").",
                        valid_actions.join(", ")
                    ));
                }
                _ => {}
            }
        }
    }

    serde_json::from_value(value).map_err(|error| {
        if let (Some(tool), Some(action_name), Some(field)) =
            (tool.as_ref(), action.as_deref(), missing_field_name(&error.to_string()))
        {
            if let Some(action_schema) = tool.actions.iter().find(|candidate| candidate.action == action_name)
            {
                let field_label = if used_flat_shorthand {
                    field.to_string()
                } else {
                    format!("input.{field}")
                };
                let shorthand_hint = if used_flat_shorthand {
                    format!(
                        " Flat shorthand was detected, so `{field}` can stay at the top level or inside `input.{field}`."
                    )
                } else {
                    String::new()
                };
                return format!(
                    "{tool_name} action `{action_name}` is missing required field `{field_label}`; required fields: {}. Inspect via prism.tool(\"{tool_name}\")?.actions.find((action) => action.action === \"{action_name}\").{shorthand_hint}",
                    action_schema.required_fields.join(", "),
                );
            }
        }

        format!(
            "invalid {tool_name} input: {}. Inspect via prism.tool(\"{tool_name}\").",
            error
        )
    })
}

fn is_flat_tagged_tool_input(value: &Value) -> bool {
    value.as_object().is_some_and(|object| {
        object.contains_key("action") && !object.contains_key("input") && object.len() > 1
    })
}

fn normalize_tagged_tool_input(mut value: Value) -> Value {
    let Some(object) = value.as_object_mut() else {
        return value;
    };
    if !is_flat_tagged_tool_input(&Value::Object(object.clone())) {
        return value;
    }

    let mut input = serde_json::Map::new();
    let keys = object
        .keys()
        .filter(|key| key.as_str() != "action")
        .cloned()
        .collect::<Vec<_>>();
    for key in keys {
        if let Some(field) = object.remove(&key) {
            input.insert(key, field);
        }
    }
    if !input.is_empty() {
        object.insert("input".to_string(), Value::Object(input));
    }
    value
}

fn missing_field_name(parse_error: &str) -> Option<&str> {
    let (_, tail) = parse_error.split_once("missing field `")?;
    let (field, _) = tail.split_once('`')?;
    Some(field)
}

fn normalize_vocab_token(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|ch| *ch != '_' && *ch != '-' && !ch.is_whitespace())
        .collect()
}

macro_rules! impl_vocab_deserialize {
    ($name:ident, $key:literal, $label:literal, $example:literal, { $($normalized:literal => $variant:ident),+ $(,)? }) => {
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let raw = String::deserialize(deserializer)?;
                let normalized = normalize_vocab_token(&raw);
                match normalized.as_str() {
                    $(
                        $normalized => Ok(Self::$variant),
                    )+
                    _ => Err(de::Error::custom(vocabulary_error(
                        $key,
                        $label,
                        raw.trim(),
                        $example,
                    ))),
                }
            }
        }
    };
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum QueryLanguage {
    Ts,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct PrismQueryArgs {
    #[schemars(description = "TypeScript snippet evaluated with a global `prism` object.")]
    pub(crate) code: String,
    #[schemars(description = "Query language. Only `ts` is currently supported.")]
    pub(crate) language: Option<QueryLanguage>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PrismLocateTaskIntentInput {
    #[serde(alias = "read", alias = "code")]
    Inspect,
    Edit,
    Validate,
    Test,
    #[serde(alias = "doc", alias = "docs", alias = "documentation", alias = "spec")]
    Explain,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismLocateArgs {
    #[schemars(description = "Search text for the compact target lookup.")]
    pub(crate) query: String,
    #[schemars(
        description = "Optional file path fragment to narrow compact locate results before ranking, for example `docs/SPEC.md` or `crates/prism-mcp/src/compact_tools.rs`."
    )]
    pub(crate) path: Option<String>,
    #[schemars(
        description = "Optional glob to narrow compact locate results before ranking, for example `docs/**` or `crates/prism-mcp/src/**`."
    )]
    pub(crate) glob: Option<String>,
    #[schemars(
        description = "Optional task intent that biases ranking toward code, docs, tests, or explanation targets. Accepts aliases such as `code` and `read` for `inspect`, plus docs-oriented aliases such as `docs` and `spec`; when omitted, docs-like `path` or `glob` filters automatically bias toward explanation targets."
    )]
    pub(crate) task_intent: Option<PrismLocateTaskIntentInput>,
    #[schemars(description = "Optional compact candidate count from 1 to 3.")]
    pub(crate) limit: Option<usize>,
    #[schemars(
        description = "When true, also include one bounded preview for the top-ranked candidate."
    )]
    pub(crate) include_top_preview: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismGatherArgs {
    #[schemars(description = "Exact text to gather as 1 to 3 bounded slices.")]
    pub(crate) query: String,
    #[schemars(
        description = "Optional file path fragment to narrow exact-text gather results, for example `benchmark_codex.py` or `docs/SPEC.md`."
    )]
    pub(crate) path: Option<String>,
    #[schemars(
        description = "Optional glob to narrow exact-text gather results, for example `benchmarks/**` or `docs/**`."
    )]
    pub(crate) glob: Option<String>,
    #[schemars(description = "Optional compact slice count from 1 to 3.")]
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum PrismOpenModeInput {
    Focus,
    Edit,
    Raw,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismOpenArgs {
    #[schemars(
        description = "Previously located compact handle to open. Exactly one of `handle` or `path` is required."
    )]
    pub(crate) handle: Option<String>,
    #[schemars(
        description = "Exact workspace file path to open directly without first minting a locate handle. Exactly one of `handle` or `path` is required."
    )]
    pub(crate) path: Option<String>,
    #[schemars(
        description = "Open mode: `focus` for a bounded local block, `edit` for an edit-oriented slice, or `raw` for the literal file window covering the target span. Path-based opens currently support only `raw`."
    )]
    pub(crate) mode: Option<PrismOpenModeInput>,
    #[schemars(
        description = "Optional 1-based focus line for exact-path opens. When present, PRISM returns a bounded window around this line."
    )]
    pub(crate) line: Option<usize>,
    #[schemars(
        description = "Optional context lines before `line` for exact-path opens. Ignored unless `line` is set."
    )]
    pub(crate) before_lines: Option<usize>,
    #[schemars(
        description = "Optional context lines after `line` for exact-path opens. Ignored unless `line` is set."
    )]
    pub(crate) after_lines: Option<usize>,
    #[schemars(description = "Optional character budget for the returned exact-path slice.")]
    pub(crate) max_chars: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismWorksetArgs {
    pub(crate) handle: Option<String>,
    pub(crate) query: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PrismExpandKindInput {
    Diagnostics,
    Lineage,
    Neighbors,
    Diff,
    Health,
    Validation,
    Impact,
    Timeline,
    Memory,
    Drift,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismExpandArgs {
    #[schemars(description = "Opaque handle returned by compact locate/open/workset.")]
    pub(crate) handle: String,
    #[schemars(description = "Requested compact expansion kind.")]
    pub(crate) kind: PrismExpandKindInput,
    #[schemars(
        description = "When true and kind is `neighbors`, also include one bounded preview for the top neighbor."
    )]
    pub(crate) include_top_preview: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismTaskBriefArgs {
    #[schemars(description = "Coordination task id to summarize through the compact task lens.")]
    pub(crate) task_id: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PrismConceptLensInput {
    Open,
    Workset,
    Validation,
    Timeline,
    Memory,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismConceptArgs {
    #[schemars(description = "Concept handle like `concept://validation_pipeline`.")]
    pub(crate) handle: Option<String>,
    #[schemars(description = "Broad repo noun or phrase to resolve into a concept packet.")]
    pub(crate) query: Option<String>,
    #[schemars(
        description = "Optional decode lens. When provided, also decode the concept into supporting context."
    )]
    pub(crate) lens: Option<PrismConceptLensInput>,
    #[schemars(
        description = "When true, include lineage-backed binding metadata aligned with the concept member lists."
    )]
    pub(crate) include_binding_metadata: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConceptMutationOperationInput {
    Promote,
    Update,
    Retire,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConceptScopeInput {
    Local,
    Session,
    Repo,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConceptRelationKindInput {
    DependsOn,
    Specializes,
    PartOf,
    ValidatedBy,
    OftenUsedWith,
    Supersedes,
    ConfusedWith,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConceptRelationMutationOperationInput {
    Upsert,
    Retire,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SparsePatchOpInput {
    Keep,
    Set,
    Clear,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SparsePatchObjectInput<T> {
    pub(crate) op: SparsePatchOpInput,
    pub(crate) value: Option<T>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(crate) enum SparsePatchInput<T> {
    Value(T),
    Patch(SparsePatchObjectInput<T>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SparsePatch<T> {
    Keep,
    Set(T),
    Clear,
}

impl<T> SparsePatchInput<T> {
    pub(crate) fn into_patch(self, field: &str) -> Result<SparsePatch<T>, String> {
        match self {
            Self::Value(value) => Ok(SparsePatch::Set(value)),
            Self::Patch(SparsePatchObjectInput { op, value }) => match op {
                SparsePatchOpInput::Keep => Ok(SparsePatch::Keep),
                SparsePatchOpInput::Set => value
                    .map(SparsePatch::Set)
                    .ok_or_else(|| format!("`{field}` patch with op `set` requires `value`")),
                SparsePatchOpInput::Clear => Ok(SparsePatch::Clear),
            },
        }
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismConceptMutationArgs {
    #[schemars(
        description = "Whether to promote a new repo concept packet or update an existing one."
    )]
    pub(crate) operation: ConceptMutationOperationInput,
    #[schemars(
        description = "Stable concept handle like `concept://validation_pipeline`. Required for `update`. Optional for `promote`; Prism derives one from `canonicalName` when omitted."
    )]
    pub(crate) handle: Option<String>,
    #[schemars(description = "Canonical repo-native concept name. Required for `promote`.")]
    pub(crate) canonical_name: Option<String>,
    #[schemars(description = "Short repo-native summary. Required for `promote`.")]
    pub(crate) summary: Option<String>,
    #[schemars(description = "Common aliases for the concept.")]
    pub(crate) aliases: Option<Vec<String>>,
    #[schemars(
        description = "2 to 5 central member nodes for the concept. Required for `promote`."
    )]
    pub(crate) core_members: Option<Vec<NodeIdInput>>,
    #[schemars(description = "Optional supporting member nodes.")]
    pub(crate) supporting_members: Option<Vec<NodeIdInput>>,
    #[schemars(description = "Optional likely test nodes.")]
    pub(crate) likely_tests: Option<Vec<NodeIdInput>>,
    #[schemars(description = "Optional evidence lines explaining why this concept exists.")]
    pub(crate) evidence: Option<Vec<String>>,
    #[schemars(description = "Optional risk hint for the concept packet.")]
    pub(crate) risk_hint: Option<SparsePatchInput<String>>,
    #[schemars(description = "Optional confidence score from 0.0 to 1.0.")]
    pub(crate) confidence: Option<f32>,
    #[schemars(description = "Optional decode lenses Prism should expose for this concept.")]
    pub(crate) decode_lenses: Option<Vec<PrismConceptLensInput>>,
    #[schemars(
        description = "Concept persistence scope. `local` stays runtime-only, `session` persists in the workspace store, and `repo` exports to committed repo knowledge."
    )]
    pub(crate) scope: Option<ConceptScopeInput>,
    #[schemars(description = "Optional concept handles this published concept supersedes.")]
    pub(crate) supersedes: Option<Vec<String>>,
    #[schemars(description = "Reason for retiring a concept. Required for `retire`.")]
    pub(crate) retirement_reason: Option<String>,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismConceptRelationMutationArgs {
    pub(crate) operation: ConceptRelationMutationOperationInput,
    pub(crate) source_handle: String,
    pub(crate) target_handle: String,
    pub(crate) kind: ConceptRelationKindInput,
    pub(crate) confidence: Option<f32>,
    pub(crate) evidence: Option<Vec<String>>,
    pub(crate) scope: Option<ConceptScopeInput>,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NodeIdInput {
    #[serde(alias = "crate_name")]
    #[serde(alias = "crateName")]
    pub(crate) crate_name: String,
    pub(crate) path: String,
    pub(crate) kind: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum AnchorRefInput {
    Node {
        #[serde(rename = "crateName", alias = "crate_name")]
        crate_name: String,
        path: String,
        kind: String,
    },
    Lineage {
        #[serde(rename = "lineageId", alias = "lineage_id")]
        lineage_id: String,
    },
    File {
        #[serde(rename = "fileId", alias = "file_id")]
        file_id: u32,
    },
    Kind {
        kind: String,
    },
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OutcomeKindInput {
    NoteAdded,
    HypothesisProposed,
    PlanCreated,
    BuildRan,
    TestRan,
    ReviewFeedback,
    FailureObserved,
    RegressionObserved,
    FixValidated,
    RollbackPerformed,
    MigrationRequired,
    IncidentLinked,
    PerfSignalObserved,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OutcomeResultInput {
    Success,
    Failure,
    Partial,
    Unknown,
}

#[derive(Debug, Clone, serde::Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MemoryKindInput {
    Episodic,
    Structural,
    Semantic,
}

#[derive(Debug, Clone, serde::Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MemorySourceInput {
    Agent,
    User,
    System,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum OutcomeEvidenceInput {
    Commit { sha: String },
    Test { name: String, passed: bool },
    Build { target: String, passed: bool },
    Reviewer { author: String },
    Issue { id: String },
    StackTrace { hash: String },
    DiffSummary { text: String },
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InferredEdgeScopeInput {
    SessionOnly,
    Persisted,
    Rejected,
    Expired,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismOutcomeArgs {
    pub(crate) kind: OutcomeKindInput,
    pub(crate) anchors: Vec<AnchorRefInput>,
    pub(crate) summary: String,
    pub(crate) result: Option<OutcomeResultInput>,
    pub(crate) evidence: Option<Vec<OutcomeEvidenceInput>>,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MemoryStorePayload {
    pub(crate) anchors: Vec<AnchorRefInput>,
    pub(crate) kind: MemoryKindInput,
    pub(crate) scope: Option<MemoryScopeInput>,
    pub(crate) content: String,
    pub(crate) trust: Option<f32>,
    pub(crate) source: Option<MemorySourceInput>,
    pub(crate) metadata: Option<Value>,
    pub(crate) promoted_from: Option<Vec<String>>,
    pub(crate) supersedes: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MemoryMutationActionInput {
    Store,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MemoryScopeInput {
    Local,
    Session,
    Repo,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ValidationFeedbackCategoryInput {
    Structural,
    Lineage,
    Memory,
    Projection,
    Coordination,
    Freshness,
    Other,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ValidationFeedbackVerdictInput {
    Wrong,
    Stale,
    Noisy,
    Helpful,
    Mixed,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismMemoryArgs {
    pub(crate) action: MemoryMutationActionInput,
    pub(crate) payload: Value,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismValidationFeedbackArgs {
    #[schemars(
        description = "Optional anchors for the feedback. Leave empty when reporting tool-level or workspace-level feedback that does not map cleanly to a semantic target."
    )]
    pub(crate) anchors: Option<Vec<AnchorRefInput>>,
    pub(crate) context: String,
    #[serde(alias = "prism_said")]
    pub(crate) prism_said: String,
    #[serde(alias = "actually_true")]
    pub(crate) actually_true: String,
    pub(crate) category: ValidationFeedbackCategoryInput,
    pub(crate) verdict: ValidationFeedbackVerdictInput,
    #[serde(alias = "corrected_manually")]
    pub(crate) corrected_manually: Option<bool>,
    pub(crate) correction: Option<String>,
    pub(crate) metadata: Option<Value>,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismStartTaskArgs {
    #[serde(alias = "label", alias = "title", alias = "summary")]
    pub(crate) description: Option<String>,
    pub(crate) tags: Option<Vec<String>>,
    #[serde(alias = "coordination_task_id")]
    pub(crate) coordination_task_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismBindCoordinationTaskArgs {
    #[serde(alias = "coordination_task_id")]
    pub(crate) coordination_task_id: String,
    pub(crate) description: Option<String>,
    pub(crate) tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismFinishTaskArgs {
    pub(crate) summary: String,
    pub(crate) anchors: Option<Vec<AnchorRefInput>>,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EventMutationResult {
    pub(crate) event_id: String,
    pub(crate) task_id: String,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MemoryMutationResult {
    pub(crate) memory_id: String,
    pub(crate) task_id: String,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ValidationFeedbackMutationResult {
    pub(crate) entry_id: String,
    pub(crate) task_id: String,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConceptMutationResult {
    pub(crate) event_id: String,
    pub(crate) concept_handle: String,
    pub(crate) task_id: String,
    pub(crate) packet: ConceptPacketView,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConceptRelationMutationResult {
    pub(crate) event_id: String,
    pub(crate) task_id: String,
    pub(crate) relation: ConceptRelationView,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EdgeMutationResult {
    pub(crate) edge_id: String,
    pub(crate) task_id: String,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CuratorProposalDecisionResult {
    pub(crate) job_id: String,
    pub(crate) proposal_index: usize,
    pub(crate) kind: String,
    pub(crate) decision: CuratorProposalDecision,
    pub(crate) proposal: Value,
    pub(crate) created: CuratorProposalCreatedResources,
    pub(crate) detail: Option<String>,
    pub(crate) memory_id: Option<String>,
    pub(crate) edge_id: Option<String>,
    pub(crate) concept_handle: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
pub(crate) enum CuratorProposalDecision {
    Applied,
    Rejected,
    NotApplicableYet,
}

#[derive(Debug, Clone, Default, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CuratorProposalCreatedResources {
    pub(crate) memory_id: Option<String>,
    pub(crate) edge_id: Option<String>,
    pub(crate) concept_handle: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
pub(crate) struct QueryDiagnosticSchema {
    pub(crate) code: String,
    pub(crate) message: String,
    pub(crate) data: Option<Value>,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
pub(crate) struct QueryEnvelopeSchema {
    pub(crate) result: Value,
    pub(crate) diagnostics: Vec<QueryDiagnosticSchema>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QueryLimitsInput {
    pub(crate) max_result_nodes: Option<usize>,
    pub(crate) max_call_graph_depth: Option<usize>,
    pub(crate) max_output_json_bytes: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismConfigureSessionArgs {
    pub(crate) limits: Option<QueryLimitsInput>,
    #[serde(alias = "current_task_id")]
    pub(crate) current_task_id: Option<String>,
    #[serde(alias = "coordination_task_id")]
    pub(crate) coordination_task_id: Option<String>,
    #[serde(alias = "current_task_description")]
    pub(crate) current_task_description: Option<String>,
    #[serde(alias = "current_task_tags")]
    pub(crate) current_task_tags: Option<Vec<String>>,
    pub(crate) clear_current_task: Option<bool>,
    #[serde(alias = "current_agent")]
    pub(crate) current_agent: Option<String>,
    pub(crate) clear_current_agent: Option<bool>,
}

#[derive(Debug, JsonSchema)]
#[schemars(transform = ensure_root_object_input_schema)]
#[serde(rename_all = "snake_case", tag = "action", content = "input")]
pub(crate) enum PrismSessionArgs {
    StartTask(PrismStartTaskArgs),
    BindCoordinationTask(PrismBindCoordinationTaskArgs),
    Configure(PrismConfigureSessionArgs),
    FinishTask(PrismFinishTaskArgs),
    AbandonTask(PrismFinishTaskArgs),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case", tag = "action", content = "input")]
enum PrismSessionArgsWire {
    StartTask(PrismStartTaskArgs),
    BindCoordinationTask(PrismBindCoordinationTaskArgs),
    Configure(PrismConfigureSessionArgs),
    FinishTask(PrismFinishTaskArgs),
    AbandonTask(PrismFinishTaskArgs),
}

impl From<PrismSessionArgsWire> for PrismSessionArgs {
    fn from(value: PrismSessionArgsWire) -> Self {
        match value {
            PrismSessionArgsWire::StartTask(args) => Self::StartTask(args),
            PrismSessionArgsWire::BindCoordinationTask(args) => Self::BindCoordinationTask(args),
            PrismSessionArgsWire::Configure(args) => Self::Configure(args),
            PrismSessionArgsWire::FinishTask(args) => Self::FinishTask(args),
            PrismSessionArgsWire::AbandonTask(args) => Self::AbandonTask(args),
        }
    }
}

impl<'de> Deserialize<'de> for PrismSessionArgs {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        parse_tagged_tool_input::<PrismSessionArgsWire>("prism_session", value)
            .map(Into::into)
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SessionMutationActionSchema {
    StartTask,
    BindCoordinationTask,
    Configure,
    FinishTask,
    AbandonTask,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismSessionMutationResult {
    pub(crate) action: SessionMutationActionSchema,
    pub(crate) task_id: Option<String>,
    pub(crate) event_id: Option<String>,
    pub(crate) memory_id: Option<String>,
    pub(crate) journal: Option<TaskJournalView>,
    pub(crate) session: SessionView,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismInferEdgeArgs {
    pub(crate) source: NodeIdInput,
    pub(crate) target: NodeIdInput,
    pub(crate) kind: String,
    pub(crate) confidence: f32,
    pub(crate) scope: Option<InferredEdgeScopeInput>,
    pub(crate) evidence: Option<Vec<String>>,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, JsonSchema)]
#[schemars(transform = ensure_root_object_input_schema)]
#[serde(rename_all = "snake_case", tag = "action", content = "input")]
pub(crate) enum PrismMutationArgs {
    Outcome(PrismOutcomeArgs),
    Memory(PrismMemoryArgs),
    Concept(PrismConceptMutationArgs),
    ConceptRelation(PrismConceptRelationMutationArgs),
    ValidationFeedback(PrismValidationFeedbackArgs),
    InferEdge(PrismInferEdgeArgs),
    Coordination(PrismCoordinationArgs),
    Claim(PrismClaimArgs),
    Artifact(PrismArtifactArgs),
    TestRan(PrismTestRanArgs),
    FailureObserved(PrismFailureObservedArgs),
    FixValidated(PrismFixValidatedArgs),
    CuratorApplyProposal(PrismCuratorApplyProposalArgs),
    CuratorPromoteEdge(PrismCuratorPromoteEdgeArgs),
    CuratorPromoteConcept(PrismCuratorPromoteConceptArgs),
    CuratorPromoteMemory(PrismCuratorPromoteMemoryArgs),
    CuratorRejectProposal(PrismCuratorRejectProposalArgs),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case", tag = "action", content = "input")]
enum PrismMutationArgsWire {
    Outcome(PrismOutcomeArgs),
    Memory(PrismMemoryArgs),
    Concept(PrismConceptMutationArgs),
    ConceptRelation(PrismConceptRelationMutationArgs),
    ValidationFeedback(PrismValidationFeedbackArgs),
    InferEdge(PrismInferEdgeArgs),
    Coordination(PrismCoordinationArgs),
    Claim(PrismClaimArgs),
    Artifact(PrismArtifactArgs),
    TestRan(PrismTestRanArgs),
    FailureObserved(PrismFailureObservedArgs),
    FixValidated(PrismFixValidatedArgs),
    CuratorApplyProposal(PrismCuratorApplyProposalArgs),
    CuratorPromoteEdge(PrismCuratorPromoteEdgeArgs),
    CuratorPromoteConcept(PrismCuratorPromoteConceptArgs),
    CuratorPromoteMemory(PrismCuratorPromoteMemoryArgs),
    CuratorRejectProposal(PrismCuratorRejectProposalArgs),
}

impl From<PrismMutationArgsWire> for PrismMutationArgs {
    fn from(value: PrismMutationArgsWire) -> Self {
        match value {
            PrismMutationArgsWire::Outcome(args) => Self::Outcome(args),
            PrismMutationArgsWire::Memory(args) => Self::Memory(args),
            PrismMutationArgsWire::Concept(args) => Self::Concept(args),
            PrismMutationArgsWire::ConceptRelation(args) => Self::ConceptRelation(args),
            PrismMutationArgsWire::ValidationFeedback(args) => Self::ValidationFeedback(args),
            PrismMutationArgsWire::InferEdge(args) => Self::InferEdge(args),
            PrismMutationArgsWire::Coordination(args) => Self::Coordination(args),
            PrismMutationArgsWire::Claim(args) => Self::Claim(args),
            PrismMutationArgsWire::Artifact(args) => Self::Artifact(args),
            PrismMutationArgsWire::TestRan(args) => Self::TestRan(args),
            PrismMutationArgsWire::FailureObserved(args) => Self::FailureObserved(args),
            PrismMutationArgsWire::FixValidated(args) => Self::FixValidated(args),
            PrismMutationArgsWire::CuratorApplyProposal(args) => Self::CuratorApplyProposal(args),
            PrismMutationArgsWire::CuratorPromoteEdge(args) => Self::CuratorPromoteEdge(args),
            PrismMutationArgsWire::CuratorPromoteConcept(args) => Self::CuratorPromoteConcept(args),
            PrismMutationArgsWire::CuratorPromoteMemory(args) => Self::CuratorPromoteMemory(args),
            PrismMutationArgsWire::CuratorRejectProposal(args) => Self::CuratorRejectProposal(args),
        }
    }
}

impl<'de> Deserialize<'de> for PrismMutationArgs {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        parse_tagged_tool_input::<PrismMutationArgsWire>("prism_mutate", value)
            .map(Into::into)
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PrismMutationActionSchema {
    Outcome,
    Memory,
    Concept,
    ConceptRelation,
    ValidationFeedback,
    InferEdge,
    Coordination,
    Claim,
    Artifact,
    TestRan,
    FailureObserved,
    FixValidated,
    CuratorApplyProposal,
    CuratorPromoteEdge,
    CuratorPromoteConcept,
    CuratorPromoteMemory,
    CuratorRejectProposal,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismMutationResult {
    pub(crate) action: PrismMutationActionSchema,
    pub(crate) result: Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismTestRanArgs {
    pub(crate) anchors: Vec<AnchorRefInput>,
    pub(crate) test: String,
    pub(crate) passed: bool,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismFailureObservedArgs {
    pub(crate) anchors: Vec<AnchorRefInput>,
    pub(crate) summary: String,
    pub(crate) trace: Option<String>,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismFixValidatedArgs {
    pub(crate) anchors: Vec<AnchorRefInput>,
    pub(crate) summary: String,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CoordinationMutationKindInput {
    PlanCreate,
    PlanUpdate,
    TaskCreate,
    TaskUpdate,
    PlanNodeCreate,
    PlanNodeUpdate,
    PlanEdgeCreate,
    PlanEdgeDelete,
    Handoff,
    HandoffAccept,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ClaimActionInput {
    Acquire,
    Renew,
    Release,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ArtifactActionInput {
    Propose,
    Supersede,
    Review,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismCoordinationArgs {
    pub(crate) kind: CoordinationMutationKindInput,
    pub(crate) payload: Value,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismClaimArgs {
    pub(crate) action: ClaimActionInput,
    pub(crate) payload: Value,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismArtifactArgs {
    pub(crate) action: ArtifactActionInput,
    pub(crate) payload: Value,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlansQueryArgs {
    pub(crate) status: Option<String>,
    pub(crate) scope: Option<String>,
    pub(crate) contains: Option<String>,
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlanTargetArgs {
    #[serde(alias = "plan_id")]
    pub(crate) plan_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlanNextArgs {
    #[serde(alias = "plan_id")]
    pub(crate) plan_id: String,
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlanNodeTargetArgs {
    #[serde(alias = "plan_id")]
    pub(crate) plan_id: String,
    #[serde(alias = "node_id")]
    pub(crate) node_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CoordinationTaskTargetArgs {
    #[serde(alias = "task_id")]
    pub(crate) task_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PolicyViolationQueryArgs {
    #[serde(alias = "plan_id")]
    pub(crate) plan_id: Option<String>,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LimitArgs {
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AnchorListArgs {
    pub(crate) anchors: Vec<AnchorRefInput>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PendingReviewsArgs {
    #[serde(alias = "plan_id")]
    pub(crate) plan_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SimulateClaimArgs {
    pub(crate) anchors: Vec<AnchorRefInput>,
    pub(crate) capability: CapabilityInput,
    pub(crate) mode: Option<ClaimModeInput>,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Clone, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum CapabilityInput {
    Observe,
    Edit,
    Review,
    Validate,
    Merge,
}

impl_vocab_deserialize!(
    CapabilityInput,
    "capability",
    "capability",
    r#"{"capability":"edit"}"#,
    {
        "observe" => Observe,
        "edit" => Edit,
        "review" => Review,
        "validate" => Validate,
        "merge" => Merge
    }
);

#[derive(Debug, Clone, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ClaimModeInput {
    Advisory,
    SoftExclusive,
    HardExclusive,
}

impl_vocab_deserialize!(
    ClaimModeInput,
    "claimMode",
    "claim mode",
    r#"{"mode":"soft_exclusive"}"#,
    {
        "advisory" => Advisory,
        "softexclusive" => SoftExclusive,
        "hardexclusive" => HardExclusive
    }
);

#[derive(Debug, Clone, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CoordinationTaskStatusInput {
    Proposed,
    Ready,
    InProgress,
    Blocked,
    InReview,
    Validating,
    Completed,
    Abandoned,
}

impl_vocab_deserialize!(
    CoordinationTaskStatusInput,
    "coordinationTaskStatus",
    "coordination task status",
    r#"{"status":"ready"}"#,
    {
        "proposed" => Proposed,
        "todo" => Ready,
        "ready" => Ready,
        "inprogress" => InProgress,
        "blocked" => Blocked,
        "inreview" => InReview,
        "validating" => Validating,
        "completed" => Completed,
        "abandoned" => Abandoned
    }
);

#[derive(Debug, Clone, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PlanStatusInput {
    Draft,
    Active,
    Blocked,
    Completed,
    Abandoned,
}

impl_vocab_deserialize!(
    PlanStatusInput,
    "planStatus",
    "coordination plan status",
    r#"{"status":"active"}"#,
    {
        "draft" => Draft,
        "active" => Active,
        "blocked" => Blocked,
        "completed" => Completed,
        "abandoned" => Abandoned
    }
);

#[derive(Debug, Clone, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AcceptanceEvidencePolicyInput {
    Any,
    All,
    ReviewOnly,
    ValidationOnly,
    ReviewAndValidation,
}

impl_vocab_deserialize!(
    AcceptanceEvidencePolicyInput,
    "acceptanceEvidencePolicy",
    "acceptance evidence policy",
    r#"{"evidencePolicy":"any"}"#,
    {
        "any" => Any,
        "all" => All,
        "reviewonly" => ReviewOnly,
        "validationonly" => ValidationOnly,
        "reviewandvalidation" => ReviewAndValidation
    }
);

#[derive(Debug, Clone, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PlanNodeStatusInput {
    Proposed,
    Ready,
    InProgress,
    Blocked,
    Waiting,
    InReview,
    Validating,
    Completed,
    Abandoned,
}

impl_vocab_deserialize!(
    PlanNodeStatusInput,
    "planNodeStatus",
    "plan node status",
    r#"{"status":"ready"}"#,
    {
        "proposed" => Proposed,
        "todo" => Ready,
        "ready" => Ready,
        "inprogress" => InProgress,
        "blocked" => Blocked,
        "waiting" => Waiting,
        "inreview" => InReview,
        "validating" => Validating,
        "completed" => Completed,
        "abandoned" => Abandoned
    }
);

#[derive(Debug, Clone, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PlanNodeKindInput {
    Investigate,
    Decide,
    Edit,
    Validate,
    Review,
    Handoff,
    Merge,
    Release,
    Note,
}

impl_vocab_deserialize!(
    PlanNodeKindInput,
    "planNodeKind",
    "plan node kind",
    r#"{"kind":"edit"}"#,
    {
        "investigate" => Investigate,
        "decide" => Decide,
        "edit" => Edit,
        "validate" => Validate,
        "review" => Review,
        "handoff" => Handoff,
        "merge" => Merge,
        "release" => Release,
        "note" => Note
    }
);

#[derive(Debug, Clone, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PlanEdgeKindInput {
    DependsOn,
    Blocks,
    Informs,
    Validates,
    HandoffTo,
    ChildOf,
    RelatedTo,
}

impl_vocab_deserialize!(
    PlanEdgeKindInput,
    "planEdgeKind",
    "plan edge kind",
    r#"{"kind":"depends_on"}"#,
    {
        "dependson" => DependsOn,
        "blocks" => Blocks,
        "informs" => Informs,
        "validates" => Validates,
        "handoffto" => HandoffTo,
        "childof" => ChildOf,
        "relatedto" => RelatedTo
    }
);

#[derive(Debug, Clone, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReviewVerdictInput {
    Approved,
    ChangesRequested,
    Rejected,
}

impl_vocab_deserialize!(
    ReviewVerdictInput,
    "reviewVerdict",
    "review verdict",
    r#"{"verdict":"approved"}"#,
    {
        "approved" => Approved,
        "changesrequested" => ChangesRequested,
        "rejected" => Rejected
    }
);

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlanCreatePayload {
    pub(crate) goal: String,
    pub(crate) status: Option<PlanStatusInput>,
    pub(crate) policy: Option<CoordinationPolicyPayload>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlanUpdatePayload {
    pub(crate) plan_id: String,
    pub(crate) status: Option<PlanStatusInput>,
    pub(crate) goal: Option<String>,
    pub(crate) policy: Option<CoordinationPolicyPayload>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CoordinationPolicyPayload {
    pub(crate) default_claim_mode: Option<ClaimModeInput>,
    pub(crate) max_parallel_editors_per_anchor: Option<u16>,
    pub(crate) require_review_for_completion: Option<bool>,
    pub(crate) require_validation_for_completion: Option<bool>,
    pub(crate) stale_after_graph_change: Option<bool>,
    pub(crate) review_required_above_risk_score: Option<f32>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ValidationRefPayload {
    pub(crate) id: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlanBindingPayload {
    pub(crate) anchors: Option<Vec<AnchorRefInput>>,
    pub(crate) concept_handles: Option<Vec<String>>,
    pub(crate) artifact_refs: Option<Vec<String>>,
    pub(crate) memory_refs: Option<Vec<String>>,
    pub(crate) outcome_refs: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AcceptanceCriterionPayload {
    pub(crate) label: String,
    pub(crate) anchors: Option<Vec<AnchorRefInput>>,
    pub(crate) required_checks: Option<Vec<ValidationRefPayload>>,
    pub(crate) evidence_policy: Option<AcceptanceEvidencePolicyInput>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskCreatePayload {
    pub(crate) plan_id: String,
    pub(crate) title: String,
    pub(crate) status: Option<CoordinationTaskStatusInput>,
    pub(crate) assignee: Option<String>,
    pub(crate) anchors: Option<Vec<AnchorRefInput>>,
    pub(crate) depends_on: Option<Vec<String>>,
    pub(crate) acceptance: Option<Vec<AcceptanceCriterionPayload>>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskUpdatePayload {
    pub(crate) task_id: String,
    pub(crate) status: Option<CoordinationTaskStatusInput>,
    pub(crate) assignee: Option<SparsePatchInput<String>>,
    pub(crate) title: Option<String>,
    pub(crate) anchors: Option<Vec<AnchorRefInput>>,
    pub(crate) depends_on: Option<Vec<String>>,
    pub(crate) acceptance: Option<Vec<AcceptanceCriterionPayload>>,
    pub(crate) completion_context: Option<TaskCompletionContextPayload>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlanNodeCreatePayload {
    pub(crate) plan_id: String,
    pub(crate) kind: Option<PlanNodeKindInput>,
    pub(crate) title: String,
    pub(crate) summary: Option<String>,
    pub(crate) status: Option<PlanNodeStatusInput>,
    pub(crate) assignee: Option<String>,
    pub(crate) is_abstract: Option<bool>,
    pub(crate) anchors: Option<Vec<AnchorRefInput>>,
    pub(crate) bindings: Option<PlanBindingPayload>,
    pub(crate) depends_on: Option<Vec<String>>,
    pub(crate) acceptance: Option<Vec<AcceptanceCriterionPayload>>,
    pub(crate) validation_refs: Option<Vec<ValidationRefPayload>>,
    pub(crate) priority: Option<u8>,
    pub(crate) tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlanNodeUpdatePayload {
    pub(crate) node_id: String,
    pub(crate) kind: Option<PlanNodeKindInput>,
    pub(crate) status: Option<PlanNodeStatusInput>,
    pub(crate) assignee: Option<SparsePatchInput<String>>,
    pub(crate) is_abstract: Option<bool>,
    pub(crate) title: Option<String>,
    pub(crate) summary: Option<SparsePatchInput<String>>,
    pub(crate) anchors: Option<Vec<AnchorRefInput>>,
    pub(crate) bindings: Option<PlanBindingPayload>,
    pub(crate) depends_on: Option<Vec<String>>,
    pub(crate) acceptance: Option<Vec<AcceptanceCriterionPayload>>,
    pub(crate) validation_refs: Option<Vec<ValidationRefPayload>>,
    pub(crate) priority: Option<SparsePatchInput<u8>>,
    pub(crate) tags: Option<Vec<String>>,
    #[allow(dead_code)]
    pub(crate) completion_context: Option<TaskCompletionContextPayload>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlanEdgeCreatePayload {
    pub(crate) plan_id: String,
    pub(crate) from_node_id: String,
    pub(crate) to_node_id: String,
    pub(crate) kind: PlanEdgeKindInput,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlanEdgeDeletePayload {
    pub(crate) plan_id: String,
    pub(crate) from_node_id: String,
    pub(crate) to_node_id: String,
    pub(crate) kind: PlanEdgeKindInput,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskCompletionContextPayload {
    pub(crate) risk_score: Option<f32>,
    pub(crate) required_validations: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HandoffPayload {
    pub(crate) task_id: String,
    pub(crate) to_agent: Option<String>,
    pub(crate) summary: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HandoffAcceptPayload {
    pub(crate) task_id: String,
    pub(crate) agent: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClaimAcquirePayload {
    pub(crate) anchors: Vec<AnchorRefInput>,
    pub(crate) capability: CapabilityInput,
    pub(crate) mode: Option<ClaimModeInput>,
    pub(crate) ttl_seconds: Option<u64>,
    pub(crate) agent: Option<String>,
    pub(crate) coordination_task_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClaimRenewPayload {
    pub(crate) claim_id: String,
    pub(crate) ttl_seconds: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClaimReleasePayload {
    pub(crate) claim_id: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ArtifactProposePayload {
    pub(crate) task_id: String,
    pub(crate) anchors: Option<Vec<AnchorRefInput>>,
    pub(crate) diff_ref: Option<String>,
    pub(crate) evidence: Option<Vec<String>>,
    pub(crate) required_validations: Option<Vec<String>>,
    pub(crate) validated_checks: Option<Vec<String>>,
    pub(crate) risk_score: Option<f32>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ArtifactSupersedePayload {
    pub(crate) artifact_id: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ArtifactReviewPayload {
    pub(crate) artifact_id: String,
    pub(crate) verdict: ReviewVerdictInput,
    pub(crate) summary: String,
    pub(crate) required_validations: Option<Vec<String>>,
    pub(crate) validated_checks: Option<Vec<String>>,
    pub(crate) risk_score: Option<f32>,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MutationViolationView {
    pub(crate) code: String,
    pub(crate) summary: String,
    pub(crate) plan_id: Option<String>,
    pub(crate) task_id: Option<String>,
    pub(crate) claim_id: Option<String>,
    pub(crate) artifact_id: Option<String>,
    pub(crate) details: Value,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CoordinationMutationResult {
    pub(crate) event_id: String,
    pub(crate) event_ids: Vec<String>,
    pub(crate) rejected: bool,
    pub(crate) violations: Vec<MutationViolationView>,
    pub(crate) state: Value,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClaimMutationResult {
    pub(crate) claim_id: Option<String>,
    pub(crate) event_ids: Vec<String>,
    pub(crate) rejected: bool,
    pub(crate) conflicts: Vec<Value>,
    pub(crate) violations: Vec<MutationViolationView>,
    pub(crate) state: Value,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ArtifactMutationResult {
    pub(crate) artifact_id: Option<String>,
    pub(crate) review_id: Option<String>,
    pub(crate) event_ids: Vec<String>,
    pub(crate) rejected: bool,
    pub(crate) violations: Vec<MutationViolationView>,
    pub(crate) state: Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismCuratorPromoteEdgeArgs {
    #[serde(alias = "job_id")]
    pub(crate) job_id: String,
    #[serde(alias = "proposal_index")]
    pub(crate) proposal_index: usize,
    pub(crate) scope: Option<InferredEdgeScopeInput>,
    pub(crate) note: Option<String>,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismCuratorApplyProposalOptionsArgs {
    pub(crate) edge_scope: Option<InferredEdgeScopeInput>,
    pub(crate) concept_scope: Option<ConceptScopeInput>,
    pub(crate) memory_trust: Option<f32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismCuratorApplyProposalArgs {
    #[serde(alias = "job_id")]
    pub(crate) job_id: String,
    #[serde(alias = "proposal_index")]
    pub(crate) proposal_index: usize,
    pub(crate) note: Option<String>,
    pub(crate) options: Option<PrismCuratorApplyProposalOptionsArgs>,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismCuratorRejectProposalArgs {
    #[serde(alias = "job_id")]
    pub(crate) job_id: String,
    #[serde(alias = "proposal_index")]
    pub(crate) proposal_index: usize,
    pub(crate) reason: Option<String>,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismCuratorPromoteConceptArgs {
    #[serde(alias = "job_id")]
    pub(crate) job_id: String,
    #[serde(alias = "proposal_index")]
    pub(crate) proposal_index: usize,
    pub(crate) scope: Option<ConceptScopeInput>,
    pub(crate) note: Option<String>,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismCuratorPromoteMemoryArgs {
    #[serde(alias = "job_id")]
    pub(crate) job_id: String,
    #[serde(alias = "proposal_index")]
    pub(crate) proposal_index: usize,
    pub(crate) trust: Option<f32>,
    pub(crate) note: Option<String>,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}
