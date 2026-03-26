use anyhow::{anyhow, Result};
use prism_agent::InferredEdgeScope;
use prism_coordination::{AcceptanceCriterion, CoordinationPolicy};
use prism_ir::{
    AnchorRef, Capability, ClaimMode, CoordinationTaskStatus, EdgeKind, EventActor, NodeId,
    NodeKind, PlanStatus, ReviewVerdict,
};
use prism_memory::{MemoryKind, OutcomeEvidence, OutcomeKind, OutcomeResult};
use serde::Deserialize;

use crate::{
    AcceptanceCriterionPayload, AnchorRefInput, CoordinationPolicyPayload, InferredEdgeScopeInput,
    MemoryKindInput, MemorySourceInput, NodeIdInput, OutcomeEvidenceInput, OutcomeKindInput,
    OutcomeResultInput, TaskCompletionContextPayload,
};

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct SymbolQueryArgs {
    pub(crate) query: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct SearchArgs {
    pub(crate) query: String,
    pub(crate) limit: Option<usize>,
    pub(crate) kind: Option<String>,
    pub(crate) path: Option<String>,
    #[serde(alias = "pathMode")]
    pub(crate) path_mode: Option<String>,
    pub(crate) strategy: Option<String>,
    #[serde(alias = "structuredPath")]
    pub(crate) structured_path: Option<String>,
    #[serde(alias = "topLevelOnly")]
    pub(crate) top_level_only: Option<bool>,
    #[serde(alias = "ownerKind")]
    pub(crate) owner_kind: Option<String>,
    #[serde(alias = "includeInferred")]
    pub(crate) include_inferred: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SymbolTargetArgs {
    pub(crate) id: Option<NodeIdInput>,
    #[serde(rename = "lineageId")]
    pub(crate) lineage_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SearchTextArgs {
    pub(crate) query: String,
    pub(crate) regex: Option<bool>,
    pub(crate) case_sensitive: Option<bool>,
    pub(crate) path: Option<String>,
    pub(crate) glob: Option<String>,
    pub(crate) limit: Option<usize>,
    pub(crate) context_lines: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QueryLogArgs {
    pub(crate) limit: Option<usize>,
    pub(crate) since: Option<u64>,
    pub(crate) target: Option<String>,
    pub(crate) operation: Option<String>,
    pub(crate) task_id: Option<String>,
    pub(crate) min_duration_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RuntimeLogArgs {
    pub(crate) limit: Option<usize>,
    pub(crate) level: Option<String>,
    pub(crate) target: Option<String>,
    pub(crate) contains: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RuntimeTimelineArgs {
    pub(crate) limit: Option<usize>,
    pub(crate) contains: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChangedFilesArgs {
    pub(crate) since: Option<u64>,
    pub(crate) limit: Option<usize>,
    pub(crate) task_id: Option<prism_ir::TaskId>,
    pub(crate) path: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChangedSymbolsArgs {
    pub(crate) path: String,
    pub(crate) since: Option<u64>,
    pub(crate) limit: Option<usize>,
    pub(crate) task_id: Option<prism_ir::TaskId>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RecentPatchesArgs {
    pub(crate) target: Option<NodeIdInput>,
    pub(crate) since: Option<u64>,
    pub(crate) limit: Option<usize>,
    pub(crate) task_id: Option<prism_ir::TaskId>,
    pub(crate) path: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiffForArgs {
    pub(crate) id: Option<NodeIdInput>,
    pub(crate) lineage_id: Option<String>,
    pub(crate) since: Option<u64>,
    pub(crate) limit: Option<usize>,
    pub(crate) task_id: Option<prism_ir::TaskId>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskChangesArgs {
    pub(crate) task_id: prism_ir::TaskId,
    pub(crate) since: Option<u64>,
    pub(crate) limit: Option<usize>,
    pub(crate) path: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct QueryTraceArgs {
    pub(crate) id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiscoveryTargetArgs {
    pub(crate) id: Option<NodeIdInput>,
    pub(crate) lineage_id: Option<String>,
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WhereUsedArgs {
    pub(crate) id: Option<NodeIdInput>,
    pub(crate) lineage_id: Option<String>,
    pub(crate) mode: Option<String>,
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImplementationTargetArgs {
    pub(crate) id: Option<NodeIdInput>,
    pub(crate) lineage_id: Option<String>,
    pub(crate) mode: Option<String>,
    pub(crate) owner_kind: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OwnerLookupArgs {
    pub(crate) id: Option<NodeIdInput>,
    pub(crate) lineage_id: Option<String>,
    pub(crate) kind: Option<String>,
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SourceExcerptArgs {
    pub(crate) id: Option<NodeIdInput>,
    pub(crate) lineage_id: Option<String>,
    pub(crate) context_lines: Option<usize>,
    pub(crate) max_lines: Option<usize>,
    pub(crate) max_chars: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EditSliceArgs {
    pub(crate) id: Option<NodeIdInput>,
    pub(crate) lineage_id: Option<String>,
    pub(crate) before_lines: Option<usize>,
    pub(crate) after_lines: Option<usize>,
    pub(crate) max_lines: Option<usize>,
    pub(crate) max_chars: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileReadArgs {
    pub(crate) path: String,
    pub(crate) start_line: Option<usize>,
    pub(crate) end_line: Option<usize>,
    pub(crate) max_chars: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileAroundArgs {
    pub(crate) path: String,
    pub(crate) line: usize,
    pub(crate) before: Option<usize>,
    pub(crate) after: Option<usize>,
    pub(crate) max_chars: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CallGraphArgs {
    pub(crate) id: Option<NodeIdInput>,
    #[serde(rename = "lineageId")]
    pub(crate) lineage_id: Option<String>,
    pub(crate) depth: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct TaskTargetArgs {
    pub(crate) task_id: prism_ir::TaskId,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskJournalArgs {
    pub(crate) task_id: prism_ir::TaskId,
    pub(crate) event_limit: Option<usize>,
    pub(crate) memory_limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MemoryRecallArgs {
    pub(crate) focus: Option<Vec<NodeIdInput>>,
    pub(crate) text: Option<String>,
    pub(crate) limit: Option<usize>,
    pub(crate) kinds: Option<Vec<String>>,
    pub(crate) since: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MemoryOutcomeArgs {
    pub(crate) focus: Option<Vec<NodeIdInput>>,
    pub(crate) task_id: Option<prism_ir::TaskId>,
    pub(crate) kinds: Option<Vec<String>>,
    pub(crate) result: Option<String>,
    pub(crate) actor: Option<String>,
    pub(crate) since: Option<u64>,
    pub(crate) limit: Option<usize>,
}

pub(crate) fn parse_memory_kind(value: &str) -> Result<MemoryKind> {
    match value.to_ascii_lowercase().as_str() {
        "episodic" | "note" | "notes" => Ok(MemoryKind::Episodic),
        "structural" | "rule" | "invariant" => Ok(MemoryKind::Structural),
        "semantic" | "summary" => Ok(MemoryKind::Semantic),
        other => Err(anyhow!("unknown memory kind `{other}`")),
    }
}

pub(crate) fn parse_outcome_kind(value: &str) -> Result<OutcomeKind> {
    match normalize_enum_label(value).as_str() {
        "noteadded" | "note" | "notes" => Ok(OutcomeKind::NoteAdded),
        "hypothesisproposed" | "hypothesis" => Ok(OutcomeKind::HypothesisProposed),
        "plancreated" | "plan" => Ok(OutcomeKind::PlanCreated),
        "patchapplied" | "patch" => Ok(OutcomeKind::PatchApplied),
        "buildran" | "build" => Ok(OutcomeKind::BuildRan),
        "testran" | "test" => Ok(OutcomeKind::TestRan),
        "reviewfeedback" | "review" => Ok(OutcomeKind::ReviewFeedback),
        "failureobserved" | "failure" => Ok(OutcomeKind::FailureObserved),
        "regressionobserved" | "regression" => Ok(OutcomeKind::RegressionObserved),
        "fixvalidated" | "validated" | "fix" => Ok(OutcomeKind::FixValidated),
        "rollbackperformed" | "rollback" => Ok(OutcomeKind::RollbackPerformed),
        "migrationrequired" | "migration" => Ok(OutcomeKind::MigrationRequired),
        "incidentlinked" | "incident" => Ok(OutcomeKind::IncidentLinked),
        "perfsignalobserved" | "perf" | "performance" => Ok(OutcomeKind::PerfSignalObserved),
        other => Err(anyhow!("unknown outcome kind `{other}`")),
    }
}

pub(crate) fn parse_outcome_result(value: &str) -> Result<OutcomeResult> {
    match normalize_enum_label(value).as_str() {
        "success" => Ok(OutcomeResult::Success),
        "failure" | "failed" => Ok(OutcomeResult::Failure),
        "partial" => Ok(OutcomeResult::Partial),
        "unknown" => Ok(OutcomeResult::Unknown),
        other => Err(anyhow!("unknown outcome result `{other}`")),
    }
}

pub(crate) fn parse_event_actor(value: &str) -> Result<EventActor> {
    let trimmed = value.trim();
    match normalize_enum_label(trimmed).as_str() {
        "user" => Ok(EventActor::User),
        "agent" => Ok(EventActor::Agent),
        "system" => Ok(EventActor::System),
        "ci" => Ok(EventActor::CI),
        _ => {
            let Some(rest) = trimmed.strip_prefix("git:") else {
                return Err(anyhow!("unknown event actor `{trimmed}`"));
            };
            let (name, email) = match rest.split_once(':') {
                Some((name, email)) => (name.trim(), Some(email.trim())),
                None => (rest.trim(), None),
            };
            if name.is_empty() {
                return Err(anyhow!("git actor must include a name"));
            }
            Ok(EventActor::GitAuthor {
                name: name.into(),
                email: email.filter(|value| !value.is_empty()).map(Into::into),
            })
        }
    }
}

fn normalize_enum_label(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct CuratorJobsArgs {
    pub(crate) status: Option<String>,
    pub(crate) trigger: Option<String>,
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct CuratorJobArgs {
    pub(crate) job_id: String,
}

pub(crate) fn convert_node_id(input: NodeIdInput) -> Result<NodeId> {
    Ok(NodeId::new(
        input.crate_name,
        input.path,
        parse_node_kind(&input.kind)?,
    ))
}

pub(crate) fn convert_anchors(inputs: Vec<AnchorRefInput>) -> Result<Vec<AnchorRef>> {
    inputs
        .into_iter()
        .map(|input| match input {
            AnchorRefInput::Node {
                crate_name,
                path,
                kind,
            } => Ok(AnchorRef::Node(NodeId::new(
                crate_name,
                path,
                parse_node_kind(&kind)?,
            ))),
            AnchorRefInput::Lineage { lineage_id } => {
                Ok(AnchorRef::Lineage(prism_ir::LineageId::new(lineage_id)))
            }
            AnchorRefInput::File { file_id } => Ok(AnchorRef::File(prism_ir::FileId(file_id))),
            AnchorRefInput::Kind { kind } => Ok(AnchorRef::Kind(parse_node_kind(&kind)?)),
        })
        .collect()
}

pub(crate) fn convert_outcome_kind(kind: OutcomeKindInput) -> OutcomeKind {
    match kind {
        OutcomeKindInput::NoteAdded => OutcomeKind::NoteAdded,
        OutcomeKindInput::HypothesisProposed => OutcomeKind::HypothesisProposed,
        OutcomeKindInput::PlanCreated => OutcomeKind::PlanCreated,
        OutcomeKindInput::BuildRan => OutcomeKind::BuildRan,
        OutcomeKindInput::TestRan => OutcomeKind::TestRan,
        OutcomeKindInput::ReviewFeedback => OutcomeKind::ReviewFeedback,
        OutcomeKindInput::FailureObserved => OutcomeKind::FailureObserved,
        OutcomeKindInput::RegressionObserved => OutcomeKind::RegressionObserved,
        OutcomeKindInput::FixValidated => OutcomeKind::FixValidated,
        OutcomeKindInput::RollbackPerformed => OutcomeKind::RollbackPerformed,
        OutcomeKindInput::MigrationRequired => OutcomeKind::MigrationRequired,
        OutcomeKindInput::IncidentLinked => OutcomeKind::IncidentLinked,
        OutcomeKindInput::PerfSignalObserved => OutcomeKind::PerfSignalObserved,
    }
}

pub(crate) fn convert_outcome_result(result: OutcomeResultInput) -> OutcomeResult {
    match result {
        OutcomeResultInput::Success => OutcomeResult::Success,
        OutcomeResultInput::Failure => OutcomeResult::Failure,
        OutcomeResultInput::Partial => OutcomeResult::Partial,
        OutcomeResultInput::Unknown => OutcomeResult::Unknown,
    }
}

pub(crate) fn convert_memory_kind(kind: MemoryKindInput) -> MemoryKind {
    match kind {
        MemoryKindInput::Episodic => MemoryKind::Episodic,
        MemoryKindInput::Structural => MemoryKind::Structural,
        MemoryKindInput::Semantic => MemoryKind::Semantic,
    }
}

pub(crate) fn convert_memory_source(source: MemorySourceInput) -> prism_memory::MemorySource {
    match source {
        MemorySourceInput::Agent => prism_memory::MemorySource::Agent,
        MemorySourceInput::User => prism_memory::MemorySource::User,
        MemorySourceInput::System => prism_memory::MemorySource::System,
    }
}

pub(crate) fn convert_outcome_evidence(evidence: OutcomeEvidenceInput) -> OutcomeEvidence {
    match evidence {
        OutcomeEvidenceInput::Commit { sha } => OutcomeEvidence::Commit { sha },
        OutcomeEvidenceInput::Test { name, passed } => OutcomeEvidence::Test { name, passed },
        OutcomeEvidenceInput::Build { target, passed } => OutcomeEvidence::Build { target, passed },
        OutcomeEvidenceInput::Reviewer { author } => OutcomeEvidence::Reviewer { author },
        OutcomeEvidenceInput::Issue { id } => OutcomeEvidence::Issue { id },
        OutcomeEvidenceInput::StackTrace { hash } => OutcomeEvidence::StackTrace { hash },
        OutcomeEvidenceInput::DiffSummary { text } => OutcomeEvidence::DiffSummary { text },
    }
}

pub(crate) fn convert_inferred_scope(scope: InferredEdgeScopeInput) -> InferredEdgeScope {
    match scope {
        InferredEdgeScopeInput::SessionOnly => InferredEdgeScope::SessionOnly,
        InferredEdgeScopeInput::Persisted => InferredEdgeScope::Persisted,
        InferredEdgeScopeInput::Rejected => InferredEdgeScope::Rejected,
        InferredEdgeScopeInput::Expired => InferredEdgeScope::Expired,
    }
}

pub(crate) fn parse_capability(value: &str) -> Result<Capability> {
    let normalized = value.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "observe" => Ok(Capability::Observe),
        "edit" => Ok(Capability::Edit),
        "review" => Ok(Capability::Review),
        "validate" => Ok(Capability::Validate),
        "merge" => Ok(Capability::Merge),
        other => Err(anyhow!("unknown capability `{other}`")),
    }
}

pub(crate) fn parse_claim_mode(value: &str) -> Result<ClaimMode> {
    let normalized = value.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "advisory" => Ok(ClaimMode::Advisory),
        "softexclusive" | "soft-exclusive" | "soft_exclusive" => Ok(ClaimMode::SoftExclusive),
        "hardexclusive" | "hard-exclusive" | "hard_exclusive" => Ok(ClaimMode::HardExclusive),
        other => Err(anyhow!("unknown claim mode `{other}`")),
    }
}

pub(crate) fn parse_coordination_task_status(value: &str) -> Result<CoordinationTaskStatus> {
    let normalized = value.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "proposed" => Ok(CoordinationTaskStatus::Proposed),
        "ready" => Ok(CoordinationTaskStatus::Ready),
        "inprogress" | "in-progress" => Ok(CoordinationTaskStatus::InProgress),
        "blocked" => Ok(CoordinationTaskStatus::Blocked),
        "inreview" | "in-review" => Ok(CoordinationTaskStatus::InReview),
        "validating" => Ok(CoordinationTaskStatus::Validating),
        "completed" => Ok(CoordinationTaskStatus::Completed),
        "abandoned" => Ok(CoordinationTaskStatus::Abandoned),
        other => Err(anyhow!("unknown coordination task status `{other}`")),
    }
}

pub(crate) fn parse_plan_status(value: &str) -> Result<PlanStatus> {
    let normalized = value.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "draft" => Ok(PlanStatus::Draft),
        "active" => Ok(PlanStatus::Active),
        "blocked" => Ok(PlanStatus::Blocked),
        "completed" => Ok(PlanStatus::Completed),
        "abandoned" => Ok(PlanStatus::Abandoned),
        other => Err(anyhow!("unknown coordination plan status `{other}`")),
    }
}

pub(crate) fn parse_review_verdict(value: &str) -> Result<ReviewVerdict> {
    let normalized = value.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "approved" => Ok(ReviewVerdict::Approved),
        "changesrequested" | "changes-requested" | "changes_requested" => {
            Ok(ReviewVerdict::ChangesRequested)
        }
        "rejected" => Ok(ReviewVerdict::Rejected),
        other => Err(anyhow!("unknown review verdict `{other}`")),
    }
}

pub(crate) fn convert_policy(
    payload: Option<CoordinationPolicyPayload>,
) -> Result<Option<CoordinationPolicy>> {
    let Some(payload) = payload else {
        return Ok(None);
    };
    let mut policy = CoordinationPolicy::default();
    if let Some(mode) = payload.default_claim_mode {
        policy.default_claim_mode = parse_claim_mode(&mode)?;
    }
    if let Some(value) = payload.max_parallel_editors_per_anchor {
        policy.max_parallel_editors_per_anchor = value;
    }
    if let Some(value) = payload.require_review_for_completion {
        policy.require_review_for_completion = value;
    }
    if let Some(value) = payload.require_validation_for_completion {
        policy.require_validation_for_completion = value;
    }
    if let Some(value) = payload.stale_after_graph_change {
        policy.stale_after_graph_change = value;
    }
    if let Some(value) = payload.review_required_above_risk_score {
        policy.review_required_above_risk_score = Some(value);
    }
    Ok(Some(policy))
}

pub(crate) fn convert_acceptance(
    payload: Option<Vec<AcceptanceCriterionPayload>>,
) -> Result<Vec<AcceptanceCriterion>> {
    payload
        .unwrap_or_default()
        .into_iter()
        .map(|criterion| {
            Ok(AcceptanceCriterion {
                label: criterion.label,
                anchors: convert_anchors(criterion.anchors.unwrap_or_default())?,
            })
        })
        .collect()
}

pub(crate) fn convert_completion_context(
    payload: Option<TaskCompletionContextPayload>,
) -> Option<prism_coordination::TaskCompletionContext> {
    payload.map(|payload| prism_coordination::TaskCompletionContext {
        risk_score: payload.risk_score,
        required_validations: payload.required_validations.unwrap_or_default(),
    })
}

pub(crate) fn parse_edge_kind(value: &str) -> Result<EdgeKind> {
    let normalized = value.trim().to_ascii_lowercase();
    let kind = match normalized.as_str() {
        "contains" => EdgeKind::Contains,
        "calls" => EdgeKind::Calls,
        "references" => EdgeKind::References,
        "implements" => EdgeKind::Implements,
        "defines" => EdgeKind::Defines,
        "imports" => EdgeKind::Imports,
        "dependson" | "depends-on" => EdgeKind::DependsOn,
        "specifies" => EdgeKind::Specifies,
        "validates" => EdgeKind::Validates,
        "relatedto" | "related-to" => EdgeKind::RelatedTo,
        other => return Err(anyhow!("unknown edge kind `{other}`")),
    };
    Ok(kind)
}

pub(crate) fn edge_kind_label(kind: EdgeKind) -> &'static str {
    match kind {
        EdgeKind::Contains => "contains",
        EdgeKind::Calls => "calls",
        EdgeKind::References => "references",
        EdgeKind::Implements => "implements",
        EdgeKind::Defines => "defines",
        EdgeKind::Imports => "imports",
        EdgeKind::DependsOn => "depends-on",
        EdgeKind::Specifies => "specifies",
        EdgeKind::Validates => "validates",
        EdgeKind::RelatedTo => "related-to",
    }
}

pub(crate) fn parse_node_kind(value: &str) -> Result<NodeKind> {
    let normalized = value.trim().to_ascii_lowercase();
    let kind = match normalized.as_str() {
        "workspace" => NodeKind::Workspace,
        "package" => NodeKind::Package,
        "document" => NodeKind::Document,
        "module" => NodeKind::Module,
        "function" => NodeKind::Function,
        "struct" => NodeKind::Struct,
        "enum" => NodeKind::Enum,
        "trait" => NodeKind::Trait,
        "impl" => NodeKind::Impl,
        "method" => NodeKind::Method,
        "field" => NodeKind::Field,
        "typealias" | "type-alias" => NodeKind::TypeAlias,
        "markdownheading" | "markdown-heading" => NodeKind::MarkdownHeading,
        "jsonkey" | "json-key" => NodeKind::JsonKey,
        "tomlkey" | "toml-key" => NodeKind::TomlKey,
        "yamlkey" | "yaml-key" => NodeKind::YamlKey,
        other => return Err(anyhow!("unknown node kind `{other}`")),
    };
    Ok(kind)
}
