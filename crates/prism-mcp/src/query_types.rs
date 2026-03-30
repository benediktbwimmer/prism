use anyhow::{anyhow, Result};
use prism_agent::InferredEdgeScope;
use prism_coordination::{AcceptanceCriterion, CoordinationPolicy};
use prism_ir::{
    AcceptanceEvidencePolicy, AnchorRef, Capability, ClaimMode, CoordinationTaskStatus, EdgeKind,
    EventActor, NodeId, NodeKind, PlanAcceptanceCriterion, PlanBinding, PlanEdgeKind, PlanNodeKind,
    PlanNodeStatus, PlanScope, PlanStatus, ReviewVerdict, ValidationRef,
};
use prism_memory::{
    MemoryEventKind, MemoryKind, MemoryScope, OutcomeEvidence, OutcomeKind, OutcomeResult,
};
use prism_query::Prism;
use serde::Deserialize;
use std::path::{Path, PathBuf};

use crate::tool_args::ValidationRefPayload;
use crate::{
    vocabulary_error, AcceptanceCriterionPayload, AcceptanceEvidencePolicyInput, AnchorRefInput,
    CapabilityInput, ClaimModeInput, CoordinationPolicyPayload, CoordinationTaskStatusInput,
    InferredEdgeScopeInput, MemoryKindInput, MemorySourceInput, NodeIdInput, OutcomeEvidenceInput,
    OutcomeKindInput, OutcomeResultInput, PlanBindingPayload, PlanEdgeKindInput, PlanNodeKindInput,
    PlanNodeStatusInput, PlanStatusInput, ReviewVerdictInput, TaskCompletionContextPayload,
};

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct SymbolQueryArgs {
    pub(crate) query: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ToolNameArgs {
    pub(crate) name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ToolValidationArgs {
    pub(crate) name: String,
    pub(crate) input: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct SearchArgs {
    pub(crate) query: String,
    pub(crate) limit: Option<usize>,
    pub(crate) kind: Option<String>,
    pub(crate) path: Option<String>,
    pub(crate) module: Option<String>,
    #[serde(alias = "taskId")]
    pub(crate) task_id: Option<String>,
    #[serde(alias = "pathMode")]
    pub(crate) path_mode: Option<String>,
    pub(crate) strategy: Option<String>,
    #[serde(alias = "structuredPath")]
    pub(crate) structured_path: Option<String>,
    #[serde(alias = "topLevelOnly")]
    pub(crate) top_level_only: Option<bool>,
    #[serde(alias = "preferCallableCode")]
    pub(crate) prefer_callable_code: Option<bool>,
    #[serde(alias = "preferEditableTargets")]
    pub(crate) prefer_editable_targets: Option<bool>,
    #[serde(alias = "preferBehavioralOwners")]
    pub(crate) prefer_behavioral_owners: Option<bool>,
    #[serde(alias = "ownerKind")]
    pub(crate) owner_kind: Option<String>,
    #[serde(alias = "includeInferred")]
    pub(crate) include_inferred: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConceptQueryArgs {
    pub(crate) query: String,
    pub(crate) limit: Option<usize>,
    pub(crate) verbosity: Option<String>,
    pub(crate) include_binding_metadata: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConceptHandleArgs {
    pub(crate) handle: String,
    pub(crate) verbosity: Option<String>,
    pub(crate) include_binding_metadata: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ContractQueryArgs {
    pub(crate) query: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DecodeConceptArgs {
    pub(crate) handle: Option<String>,
    pub(crate) query: Option<String>,
    pub(crate) lens: String,
    pub(crate) verbosity: Option<String>,
    pub(crate) include_binding_metadata: Option<bool>,
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
pub(crate) struct McpLogArgs {
    pub(crate) limit: Option<usize>,
    pub(crate) since: Option<u64>,
    pub(crate) call_type: Option<String>,
    pub(crate) name: Option<String>,
    pub(crate) task_id: Option<String>,
    pub(crate) session_id: Option<String>,
    pub(crate) success: Option<bool>,
    pub(crate) min_duration_ms: Option<u64>,
    pub(crate) contains: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct McpTraceArgs {
    pub(crate) id: String,
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
pub(crate) struct ValidationFeedbackArgs {
    pub(crate) limit: Option<usize>,
    pub(crate) since: Option<u64>,
    pub(crate) task_id: Option<String>,
    pub(crate) verdict: Option<String>,
    pub(crate) category: Option<String>,
    pub(crate) contains: Option<String>,
    pub(crate) corrected_manually: Option<bool>,
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

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileReadArgs {
    pub(crate) path: String,
    pub(crate) start_line: Option<usize>,
    pub(crate) end_line: Option<usize>,
    pub(crate) max_chars: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MemoryEventArgs {
    pub(crate) memory_id: Option<String>,
    pub(crate) focus: Option<Vec<NodeIdInput>>,
    pub(crate) text: Option<String>,
    pub(crate) limit: Option<usize>,
    pub(crate) kinds: Option<Vec<String>>,
    pub(crate) actions: Option<Vec<String>>,
    pub(crate) scope: Option<String>,
    pub(crate) task_id: Option<String>,
    pub(crate) since: Option<u64>,
}

pub(crate) fn parse_memory_kind(value: &str) -> Result<MemoryKind> {
    match value.to_ascii_lowercase().as_str() {
        "episodic" | "note" | "notes" => Ok(MemoryKind::Episodic),
        "structural" | "rule" | "invariant" => Ok(MemoryKind::Structural),
        "semantic" | "summary" => Ok(MemoryKind::Semantic),
        other => Err(anyhow!("unknown memory kind `{other}`")),
    }
}

pub(crate) fn parse_memory_scope(value: &str) -> Result<MemoryScope> {
    match value.to_ascii_lowercase().as_str() {
        "local" | "private" | "machine" => Ok(MemoryScope::Local),
        "session" | "workspace" => Ok(MemoryScope::Session),
        "repo" | "shared" => Ok(MemoryScope::Repo),
        other => Err(anyhow!("unknown memory scope `{other}`")),
    }
}

pub(crate) fn parse_memory_event_action(value: &str) -> Result<MemoryEventKind> {
    match value.to_ascii_lowercase().as_str() {
        "stored" | "store" => Ok(MemoryEventKind::Stored),
        "promoted" | "promote" => Ok(MemoryEventKind::Promoted),
        "superseded" | "supersede" => Ok(MemoryEventKind::Superseded),
        "retired" | "retire" => Ok(MemoryEventKind::Retired),
        other => Err(anyhow!("unknown memory event action `{other}`")),
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
#[serde(rename_all = "camelCase")]
pub(crate) struct CuratorProposalsArgs {
    pub(crate) status: Option<String>,
    pub(crate) trigger: Option<String>,
    pub(crate) kind: Option<String>,
    pub(crate) disposition: Option<String>,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
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

fn resolve_anchor_file_path(path: &str, workspace_root: Option<&Path>) -> Result<PathBuf> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("file anchors require a non-empty `path`"));
    }
    let candidate = PathBuf::from(trimmed);
    if candidate.is_absolute() {
        return Ok(candidate);
    }
    let root = workspace_root
        .ok_or_else(|| anyhow!("relative file anchors require a workspace-backed PRISM session"))?;
    Ok(root.join(candidate))
}

pub(crate) fn convert_anchors(
    prism: &Prism,
    workspace_root: Option<&Path>,
    inputs: Vec<AnchorRefInput>,
) -> Result<Vec<AnchorRef>> {
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
            AnchorRefInput::File { file_id, path } => {
                let resolved_from_path = match path {
                    Some(path) => {
                        let file_path = resolve_anchor_file_path(&path, workspace_root)?;
                        let file_id = prism.graph().file_id(&file_path).ok_or_else(|| {
                            anyhow!(
                                "file anchor path `{}` does not match any indexed workspace file",
                                file_path.display()
                            )
                        })?;
                        Some((file_path, file_id))
                    }
                    None => None,
                };
                match (file_id, resolved_from_path) {
                    (Some(file_id), Some((file_path, resolved_file_id))) => {
                        if resolved_file_id.0 != file_id {
                            return Err(anyhow!(
                                "file anchor `fileId` {} does not match resolved path `{}` (expected fileId {}).",
                                file_id,
                                file_path.display(),
                                resolved_file_id.0
                            ));
                        }
                        Ok(AnchorRef::File(prism_ir::FileId(file_id)))
                    }
                    (Some(file_id), None) => Ok(AnchorRef::File(prism_ir::FileId(file_id))),
                    (None, Some((_, resolved_file_id))) => Ok(AnchorRef::File(resolved_file_id)),
                    (None, None) => Err(anyhow!(
                        "file anchors require either `fileId` or `path`"
                    )),
                }
            }
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

pub(crate) fn convert_memory_scope(scope: crate::MemoryScopeInput) -> MemoryScope {
    match scope {
        crate::MemoryScopeInput::Local => MemoryScope::Local,
        crate::MemoryScopeInput::Session => MemoryScope::Session,
        crate::MemoryScopeInput::Repo => MemoryScope::Repo,
    }
}

pub(crate) fn convert_outcome_evidence(evidence: OutcomeEvidenceInput) -> OutcomeEvidence {
    match evidence {
        OutcomeEvidenceInput::Commit { sha } => OutcomeEvidence::Commit { sha },
        OutcomeEvidenceInput::Test { name, passed } => OutcomeEvidence::Test { name, passed },
        OutcomeEvidenceInput::Build { target, passed } => OutcomeEvidence::Build { target, passed },
        OutcomeEvidenceInput::Command { argv, passed } => OutcomeEvidence::Command { argv, passed },
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

pub(crate) fn convert_capability(value: CapabilityInput) -> Capability {
    match value {
        CapabilityInput::Observe => Capability::Observe,
        CapabilityInput::Edit => Capability::Edit,
        CapabilityInput::Review => Capability::Review,
        CapabilityInput::Validate => Capability::Validate,
        CapabilityInput::Merge => Capability::Merge,
    }
}

pub(crate) fn convert_claim_mode(value: ClaimModeInput) -> ClaimMode {
    match value {
        ClaimModeInput::Advisory => ClaimMode::Advisory,
        ClaimModeInput::SoftExclusive => ClaimMode::SoftExclusive,
        ClaimModeInput::HardExclusive => ClaimMode::HardExclusive,
    }
}

pub(crate) fn convert_coordination_task_status(
    value: CoordinationTaskStatusInput,
) -> CoordinationTaskStatus {
    match value {
        CoordinationTaskStatusInput::Proposed => CoordinationTaskStatus::Proposed,
        CoordinationTaskStatusInput::Ready => CoordinationTaskStatus::Ready,
        CoordinationTaskStatusInput::InProgress => CoordinationTaskStatus::InProgress,
        CoordinationTaskStatusInput::Blocked => CoordinationTaskStatus::Blocked,
        CoordinationTaskStatusInput::InReview => CoordinationTaskStatus::InReview,
        CoordinationTaskStatusInput::Validating => CoordinationTaskStatus::Validating,
        CoordinationTaskStatusInput::Completed => CoordinationTaskStatus::Completed,
        CoordinationTaskStatusInput::Abandoned => CoordinationTaskStatus::Abandoned,
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
        other => Err(anyhow!(vocabulary_error(
            "planStatus",
            "coordination plan status",
            other,
            r#"{"status":"active"}"#
        ))),
    }
}

pub(crate) fn convert_plan_status(value: PlanStatusInput) -> PlanStatus {
    match value {
        PlanStatusInput::Draft => PlanStatus::Draft,
        PlanStatusInput::Active => PlanStatus::Active,
        PlanStatusInput::Blocked => PlanStatus::Blocked,
        PlanStatusInput::Completed => PlanStatus::Completed,
        PlanStatusInput::Abandoned => PlanStatus::Abandoned,
    }
}

pub(crate) fn parse_plan_scope(value: &str) -> Result<PlanScope> {
    let normalized = value.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "local" => Ok(PlanScope::Local),
        "session" => Ok(PlanScope::Session),
        "repo" => Ok(PlanScope::Repo),
        other => Err(anyhow!(vocabulary_error(
            "planScope",
            "plan scope",
            other,
            r#"{"scope":"repo"}"#
        ))),
    }
}

pub(crate) fn convert_plan_node_status(value: PlanNodeStatusInput) -> PlanNodeStatus {
    match value {
        PlanNodeStatusInput::Proposed => PlanNodeStatus::Proposed,
        PlanNodeStatusInput::Ready => PlanNodeStatus::Ready,
        PlanNodeStatusInput::InProgress => PlanNodeStatus::InProgress,
        PlanNodeStatusInput::Blocked => PlanNodeStatus::Blocked,
        PlanNodeStatusInput::Waiting => PlanNodeStatus::Waiting,
        PlanNodeStatusInput::InReview => PlanNodeStatus::InReview,
        PlanNodeStatusInput::Validating => PlanNodeStatus::Validating,
        PlanNodeStatusInput::Completed => PlanNodeStatus::Completed,
        PlanNodeStatusInput::Abandoned => PlanNodeStatus::Abandoned,
    }
}

pub(crate) fn convert_plan_node_kind(value: PlanNodeKindInput) -> PlanNodeKind {
    match value {
        PlanNodeKindInput::Investigate => PlanNodeKind::Investigate,
        PlanNodeKindInput::Decide => PlanNodeKind::Decide,
        PlanNodeKindInput::Edit => PlanNodeKind::Edit,
        PlanNodeKindInput::Validate => PlanNodeKind::Validate,
        PlanNodeKindInput::Review => PlanNodeKind::Review,
        PlanNodeKindInput::Handoff => PlanNodeKind::Handoff,
        PlanNodeKindInput::Merge => PlanNodeKind::Merge,
        PlanNodeKindInput::Release => PlanNodeKind::Release,
        PlanNodeKindInput::Note => PlanNodeKind::Note,
    }
}

pub(crate) fn convert_plan_edge_kind(value: PlanEdgeKindInput) -> PlanEdgeKind {
    match value {
        PlanEdgeKindInput::DependsOn => PlanEdgeKind::DependsOn,
        PlanEdgeKindInput::Blocks => PlanEdgeKind::Blocks,
        PlanEdgeKindInput::Informs => PlanEdgeKind::Informs,
        PlanEdgeKindInput::Validates => PlanEdgeKind::Validates,
        PlanEdgeKindInput::HandoffTo => PlanEdgeKind::HandoffTo,
        PlanEdgeKindInput::ChildOf => PlanEdgeKind::ChildOf,
        PlanEdgeKindInput::RelatedTo => PlanEdgeKind::RelatedTo,
    }
}

pub(crate) fn convert_review_verdict(value: ReviewVerdictInput) -> ReviewVerdict {
    match value {
        ReviewVerdictInput::Approved => ReviewVerdict::Approved,
        ReviewVerdictInput::ChangesRequested => ReviewVerdict::ChangesRequested,
        ReviewVerdictInput::Rejected => ReviewVerdict::Rejected,
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
        policy.default_claim_mode = convert_claim_mode(mode);
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
    prism: &Prism,
    workspace_root: Option<&Path>,
    payload: Option<Vec<AcceptanceCriterionPayload>>,
) -> Result<Vec<AcceptanceCriterion>> {
    payload
        .unwrap_or_default()
        .into_iter()
        .map(|criterion| {
            Ok(AcceptanceCriterion {
                label: criterion.label,
                anchors: convert_anchors(
                    prism,
                    workspace_root,
                    criterion.anchors.unwrap_or_default(),
                )?,
            })
        })
        .collect()
}

pub(crate) fn convert_plan_acceptance(
    prism: &Prism,
    workspace_root: Option<&Path>,
    payload: Option<Vec<AcceptanceCriterionPayload>>,
) -> Result<Vec<PlanAcceptanceCriterion>> {
    payload
        .unwrap_or_default()
        .into_iter()
        .map(|criterion| {
            Ok(PlanAcceptanceCriterion {
                label: criterion.label,
                anchors: convert_anchors(
                    prism,
                    workspace_root,
                    criterion.anchors.unwrap_or_default(),
                )?,
                required_checks: criterion
                    .required_checks
                    .unwrap_or_default()
                    .into_iter()
                    .map(|check| ValidationRef { id: check.id })
                    .collect(),
                evidence_policy: criterion
                    .evidence_policy
                    .map(convert_acceptance_evidence_policy)
                    .unwrap_or(AcceptanceEvidencePolicy::Any),
            })
        })
        .collect()
}

pub(crate) fn convert_validation_refs(
    payload: Option<Vec<ValidationRefPayload>>,
) -> Vec<ValidationRef> {
    payload
        .unwrap_or_default()
        .into_iter()
        .map(|check| ValidationRef { id: check.id })
        .collect()
}

pub(crate) fn convert_plan_binding(
    prism: &Prism,
    workspace_root: Option<&Path>,
    explicit_anchors: Option<Vec<AnchorRefInput>>,
    payload: Option<PlanBindingPayload>,
) -> Result<Option<PlanBinding>> {
    if explicit_anchors.is_none() && payload.is_none() {
        return Ok(None);
    }
    let mut binding = PlanBinding::default();
    if let Some(payload) = payload {
        binding.anchors =
            convert_anchors(prism, workspace_root, payload.anchors.unwrap_or_default())?;
        binding.concept_handles = payload.concept_handles.unwrap_or_default();
        binding.artifact_refs = payload.artifact_refs.unwrap_or_default();
        binding.memory_refs = payload.memory_refs.unwrap_or_default();
        binding.outcome_refs = payload.outcome_refs.unwrap_or_default();
    }
    if let Some(explicit) = explicit_anchors {
        binding
            .anchors
            .extend(convert_anchors(prism, workspace_root, explicit)?);
    }
    Ok(Some(binding))
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

pub(crate) fn convert_acceptance_evidence_policy(
    value: AcceptanceEvidencePolicyInput,
) -> AcceptanceEvidencePolicy {
    match value {
        AcceptanceEvidencePolicyInput::Any => AcceptanceEvidencePolicy::Any,
        AcceptanceEvidencePolicyInput::All => AcceptanceEvidencePolicy::All,
        AcceptanceEvidencePolicyInput::ReviewOnly => AcceptanceEvidencePolicy::ReviewOnly,
        AcceptanceEvidencePolicyInput::ValidationOnly => AcceptanceEvidencePolicy::ValidationOnly,
        AcceptanceEvidencePolicyInput::ReviewAndValidation => {
            AcceptanceEvidencePolicy::ReviewAndValidation
        }
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
