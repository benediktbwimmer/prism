use std::sync::atomic::Ordering;

use anyhow::{anyhow, Result};
use prism_coordination::{
    HandoffAcceptInput, HandoffInput, PolicyViolation, TaskCreateInput, TaskUpdateInput,
};
use prism_core::{ValidationFeedbackCategory, ValidationFeedbackRecord, ValidationFeedbackVerdict};
use prism_curator::{
    CandidateConcept, CandidateConceptOperation, CuratorJobId, CuratorProposal,
    CuratorProposalDisposition,
};
use prism_ir::{
    new_prefixed_id, AgentId, AnchorRef, ArtifactId, ClaimId, CoordinationTaskId, Edge, EdgeOrigin,
    EventActor, EventId, EventMeta, PlanEdge, PlanEdgeId, PlanEdgeKind, PlanId, PlanNodeId, TaskId,
};
use prism_js::{CuratorProposalRecordView, TaskJournalView};
use prism_memory::{
    MemoryEntry, MemoryEvent, MemoryEventKind, MemoryKind, MemoryModule, MemoryScope, MemorySource,
    OutcomeEvent, OutcomeEvidence, OutcomeKind, OutcomeResult,
};
use prism_query::{
    canonical_concept_handle, canonical_contract_handle, ConceptEvent, ConceptEventAction,
    ConceptEventPatch, ConceptPacket, ConceptProvenance, ConceptPublication,
    ConceptPublicationStatus, ConceptRelation, ConceptRelationEvent, ConceptRelationEventAction,
    ConceptRelationKind, ConceptScope, ContractCompatibility, ContractEvent, ContractEventAction,
    ContractEventPatch, ContractGuarantee, ContractGuaranteeStrength, ContractKind, ContractPacket,
    ContractStability, ContractStatus, ContractTarget, ContractValidation, Prism,
};
use serde_json::{json, Value};

use crate::dashboard_events::MutationRun;
use crate::{
    artifact_view, claim_view, concept_packet_view, concept_relation_view, conflict_view,
    contract_packet_view, convert_acceptance, convert_anchors, convert_capability,
    convert_claim_mode, convert_completion_context, convert_coordination_task_status,
    convert_inferred_scope, convert_memory_kind, convert_memory_scope, convert_memory_source,
    convert_node_id, convert_outcome_evidence, convert_outcome_kind, convert_outcome_result,
    convert_plan_acceptance, convert_plan_binding, convert_plan_edge_kind, convert_plan_node_kind,
    convert_plan_node_status, convert_plan_status, convert_policy, convert_review_verdict,
    convert_validation_refs, coordination_task_view, curator_disposition_label,
    curator_job_status_label, curator_memory_metadata, curator_proposal, curator_proposal_state,
    curator_trigger_label, current_timestamp, ensure_repo_publication_metadata,
    manual_memory_metadata, parse_edge_kind, plan_edge_view, plan_node_view, plan_view,
    retire_repo_publication_metadata, task_journal_memory_metadata, ArtifactActionInput,
    ArtifactMutationResult, ArtifactProposePayload, ArtifactReviewPayload,
    ArtifactSupersedePayload, ClaimAcquirePayload, ClaimActionInput, ClaimMutationResult,
    ClaimReleasePayload, ClaimRenewPayload, ConceptMutationOperationInput, ConceptMutationResult,
    ConceptRelationKindInput, ConceptRelationMutationOperationInput, ConceptRelationMutationResult,
    ConceptScopeInput, ConceptVerbosity, ContractCompatibilityInput, ContractGuaranteeInput,
    ContractGuaranteeStrengthInput, ContractKindInput, ContractMutationOperationInput,
    ContractMutationResult, ContractStabilityInput, ContractStatusInput, ContractTargetInput,
    ContractValidationInput, CoordinationMutationKindInput, CoordinationMutationResult,
    CuratorJobView, CuratorProposalCreatedResources, CuratorProposalDecision,
    CuratorProposalDecisionResult, EdgeMutationResult, EventMutationResult, HandoffAcceptPayload,
    MemoryMutationActionInput, MemoryMutationResult, MemoryRetirePayload, MemoryStorePayload,
    MutationViolationView, NodeIdInput, PlanEdgeCreatePayload, PlanEdgeDeletePayload,
    PlanNodeCreatePayload, PlanUpdatePayload, PrismArtifactArgs, PrismClaimArgs,
    PrismConceptLensInput, PrismConceptMutationArgs, PrismConceptRelationMutationArgs,
    PrismContractMutationArgs, PrismCoordinationArgs, PrismCuratorApplyProposalArgs,
    PrismCuratorPromoteConceptArgs, PrismCuratorPromoteEdgeArgs, PrismCuratorPromoteMemoryArgs,
    PrismCuratorRejectProposalArgs, PrismFinishTaskArgs, PrismInferEdgeArgs, PrismMemoryArgs,
    PrismOutcomeArgs, PrismValidationFeedbackArgs, QueryHost, SessionState, SparsePatch,
    SparsePatchInput, TaskCreatePayload, ValidationFeedbackCategoryInput,
    ValidationFeedbackMutationResult, ValidationFeedbackVerdictInput, WorkflowStatusInput,
    WorkflowUpdatePayload, DEFAULT_TASK_JOURNAL_EVENT_LIMIT, DEFAULT_TASK_JOURNAL_MEMORY_LIMIT,
};

#[derive(Default)]
struct CoordinationAudit {
    event_ids: Vec<String>,
    violations: Vec<MutationViolationView>,
    rejected: bool,
}

#[derive(Clone, serde::Serialize)]
pub(crate) struct TaskClosureMutationResult {
    pub(crate) task_id: String,
    pub(crate) event_id: String,
    pub(crate) memory_id: String,
    pub(crate) journal: TaskJournalView,
}

enum TaskClosureDisposition {
    Completed,
    Abandoned,
}

impl TaskClosureDisposition {
    fn label(&self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Abandoned => "abandoned",
        }
    }

    fn outcome_result(&self) -> OutcomeResult {
        match self {
            Self::Completed => OutcomeResult::Success,
            Self::Abandoned => OutcomeResult::Partial,
        }
    }

    fn trust(&self) -> f32 {
        match self {
            Self::Completed => 0.85,
            Self::Abandoned => 0.7,
        }
    }
}

fn mutation_violation_view(value: PolicyViolation) -> MutationViolationView {
    MutationViolationView {
        code: serde_json::to_string(&value.code)
            .unwrap_or_else(|_| "\"unknown\"".to_string())
            .trim_matches('"')
            .to_string(),
        summary: value.summary,
        plan_id: value.plan_id.map(|id| id.0.to_string()),
        task_id: value.task_id.map(|id| id.0.to_string()),
        claim_id: value.claim_id.map(|id| id.0.to_string()),
        artifact_id: value.artifact_id.map(|id| id.0.to_string()),
        details: value.details,
    }
}

fn coordination_audit_since(prism: &Prism, before_len: usize) -> CoordinationAudit {
    let mut audit = CoordinationAudit::default();
    for event in prism.coordination_events().into_iter().skip(before_len) {
        audit.event_ids.push(event.meta.id.0.to_string());
        if event.kind == prism_ir::CoordinationEventKind::MutationRejected {
            audit.rejected = true;
        }
        if let Some(value) = event.metadata.get("violations") {
            if let Ok(violations) = serde_json::from_value::<Vec<PolicyViolation>>(value.clone()) {
                audit
                    .violations
                    .extend(violations.into_iter().map(mutation_violation_view));
            }
        }
    }
    audit
}

fn current_plan_node_state(prism: &Prism, plan_id: &PlanId, node_id: &str) -> Result<Value> {
    let graph = prism
        .plan_graph(plan_id)
        .ok_or_else(|| anyhow!("unknown plan `{}`", plan_id.0))?;
    let node = graph
        .nodes
        .into_iter()
        .find(|node| node.id.0 == node_id)
        .ok_or_else(|| anyhow!("unknown plan node `{node_id}`"))?;
    Ok(serde_json::to_value(plan_node_view(node))?)
}

fn current_plan_edge_state(
    prism: &Prism,
    plan_id: &PlanId,
    from_node_id: &str,
    to_node_id: &str,
    kind: PlanEdgeKind,
) -> Result<Value> {
    let graph = prism
        .plan_graph(plan_id)
        .ok_or_else(|| anyhow!("unknown plan `{}`", plan_id.0))?;
    let edge = graph
        .edges
        .into_iter()
        .find(|edge| edge.from.0 == from_node_id && edge.to.0 == to_node_id && edge.kind == kind)
        .ok_or_else(|| {
            anyhow!(
                "unknown plan edge `{}` -> `{}` ({:?})",
                from_node_id,
                to_node_id,
                kind
            )
        })?;
    Ok(serde_json::to_value(plan_edge_view(edge))?)
}

fn deleted_plan_edge_state(
    plan_id: &PlanId,
    from_node_id: &str,
    to_node_id: &str,
    kind: PlanEdgeKind,
) -> Result<Value> {
    Ok(serde_json::to_value(plan_edge_view(PlanEdge {
        id: PlanEdgeId::new(format!(
            "plan-edge:{}:{}:{}",
            from_node_id,
            plan_edge_kind_slug(kind),
            to_node_id
        )),
        plan_id: plan_id.clone(),
        from: PlanNodeId::new(from_node_id.to_string()),
        to: PlanNodeId::new(to_node_id.to_string()),
        kind,
        summary: None,
        metadata: Value::Null,
    }))?)
}

fn plan_edge_kind_slug(kind: PlanEdgeKind) -> &'static str {
    match kind {
        PlanEdgeKind::DependsOn => "depends-on",
        PlanEdgeKind::Blocks => "blocks",
        PlanEdgeKind::Informs => "informs",
        PlanEdgeKind::Validates => "validates",
        PlanEdgeKind::HandoffTo => "handoff-to",
        PlanEdgeKind::ChildOf => "child-of",
        PlanEdgeKind::RelatedTo => "related-to",
    }
}

fn resolve_native_plan_node(prism: &Prism, node_id: &str) -> Option<(PlanId, prism_ir::PlanNode)> {
    prism.plan_graphs().into_iter().find_map(|graph| {
        graph
            .nodes
            .into_iter()
            .find_map(|node| (node.id.0 == node_id).then(|| (graph.id.clone(), node)))
    })
}

enum WorkflowUpdateTarget {
    CoordinationTask(CoordinationTaskId),
    PlanNode {
        plan_id: PlanId,
        node_id: PlanNodeId,
    },
}

fn resolve_workflow_update_target_with_preference(
    prism: &Prism,
    id: &str,
    prefer_plan_node: bool,
) -> Result<WorkflowUpdateTarget> {
    let task_id = CoordinationTaskId::new(id.to_string());
    let plan_node = resolve_native_plan_node(prism, id);
    if prefer_plan_node {
        if let Some((plan_id, node)) = plan_node.clone() {
            return Ok(WorkflowUpdateTarget::PlanNode {
                plan_id,
                node_id: node.id,
            });
        }
    }
    if prism.coordination_task(&task_id).is_some() {
        return Ok(WorkflowUpdateTarget::CoordinationTask(task_id));
    }
    if let Some((plan_id, node)) = plan_node {
        return Ok(WorkflowUpdateTarget::PlanNode {
            plan_id,
            node_id: node.id,
        });
    }
    Err(anyhow!("unknown coordination task or plan node `{id}`"))
}

fn convert_workflow_status_for_task(
    value: WorkflowStatusInput,
) -> Result<prism_ir::CoordinationTaskStatus> {
    match value {
        WorkflowStatusInput::Proposed => Ok(prism_ir::CoordinationTaskStatus::Proposed),
        WorkflowStatusInput::Ready => Ok(prism_ir::CoordinationTaskStatus::Ready),
        WorkflowStatusInput::InProgress => Ok(prism_ir::CoordinationTaskStatus::InProgress),
        WorkflowStatusInput::Blocked => Ok(prism_ir::CoordinationTaskStatus::Blocked),
        WorkflowStatusInput::Waiting => Err(anyhow!(
            "status `waiting` is only supported for native plan nodes"
        )),
        WorkflowStatusInput::InReview => Ok(prism_ir::CoordinationTaskStatus::InReview),
        WorkflowStatusInput::Validating => Ok(prism_ir::CoordinationTaskStatus::Validating),
        WorkflowStatusInput::Completed => Ok(prism_ir::CoordinationTaskStatus::Completed),
        WorkflowStatusInput::Abandoned => Ok(prism_ir::CoordinationTaskStatus::Abandoned),
    }
}

fn convert_workflow_status_for_plan_node(value: WorkflowStatusInput) -> prism_ir::PlanNodeStatus {
    match value {
        WorkflowStatusInput::Proposed => prism_ir::PlanNodeStatus::Proposed,
        WorkflowStatusInput::Ready => prism_ir::PlanNodeStatus::Ready,
        WorkflowStatusInput::InProgress => prism_ir::PlanNodeStatus::InProgress,
        WorkflowStatusInput::Blocked => prism_ir::PlanNodeStatus::Blocked,
        WorkflowStatusInput::Waiting => prism_ir::PlanNodeStatus::Waiting,
        WorkflowStatusInput::InReview => prism_ir::PlanNodeStatus::InReview,
        WorkflowStatusInput::Validating => prism_ir::PlanNodeStatus::Validating,
        WorkflowStatusInput::Completed => prism_ir::PlanNodeStatus::Completed,
        WorkflowStatusInput::Abandoned => prism_ir::PlanNodeStatus::Abandoned,
    }
}

impl QueryHost {
    fn ensure_tool_enabled(&self, tool_name: &str, label: &str) -> Result<()> {
        if !self.features.is_tool_enabled(tool_name) {
            return Err(anyhow!(
                "{label} are disabled by the PRISM MCP server feature flags"
            ));
        }
        Ok(())
    }

    pub(crate) fn start_task(
        &self,
        session: &SessionState,
        description: Option<String>,
        tags: Vec<String>,
        coordination_task_id: Option<String>,
    ) -> Result<TaskId> {
        let (task, description, coordination_task_id) =
            if let Some(coordination_task_id) = coordination_task_id {
                let coordination_task = self
                    .current_prism()
                    .coordination_task(&prism_ir::CoordinationTaskId::new(
                        coordination_task_id.clone(),
                    ))
                    .ok_or_else(|| anyhow!("unknown coordination task `{coordination_task_id}`"))?;
                let description = description
                    .map(|value| value.trim().to_owned())
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| coordination_task.title.clone());
                (
                    session.start_task(
                        &description,
                        &tags,
                        Some(TaskId::new(coordination_task_id.clone())),
                        Some(coordination_task_id.clone()),
                    ),
                    description,
                    Some(coordination_task_id),
                )
            } else {
                let description = description.unwrap_or_default();
                (
                    session.start_task(&description, &tags, None, None),
                    description,
                    None,
                )
            };
        let event = OutcomeEvent {
            meta: EventMeta {
                id: session.next_event_id("outcome"),
                ts: current_timestamp(),
                actor: EventActor::Agent,
                correlation: Some(task.clone()),
                causation: None,
            },
            anchors: Vec::new(),
            kind: prism_memory::OutcomeKind::PlanCreated,
            result: prism_memory::OutcomeResult::Success,
            summary: description,
            evidence: Vec::new(),
            metadata: json!({
                "tags": tags,
                "coordinationTaskId": coordination_task_id,
            }),
        };
        if let Some(workspace) = &self.workspace {
            if workspace.try_append_outcome(event)?.is_some() {
                self.sync_workspace_revision(workspace)?;
            }
        } else {
            let prism = self.current_prism();
            prism.apply_outcome_event_to_projections(&event);
            let _ = prism.outcome_memory().store_event(event)?;
            self.persist_outcomes()?;
        }
        self.persist_session_seed(session)?;
        Ok(task)
    }

    #[allow(dead_code)]
    pub(crate) fn finish_task(
        &self,
        session: &SessionState,
        args: PrismFinishTaskArgs,
    ) -> Result<TaskClosureMutationResult> {
        self.close_task_without_refresh(session, args, TaskClosureDisposition::Completed)
    }

    #[allow(dead_code)]
    pub(crate) fn abandon_task(
        &self,
        session: &SessionState,
        args: PrismFinishTaskArgs,
    ) -> Result<TaskClosureMutationResult> {
        self.close_task_without_refresh(session, args, TaskClosureDisposition::Abandoned)
    }

    pub(crate) fn finish_task_without_refresh(
        &self,
        session: &SessionState,
        args: PrismFinishTaskArgs,
    ) -> Result<TaskClosureMutationResult> {
        self.close_task_without_refresh(session, args, TaskClosureDisposition::Completed)
    }

    pub(crate) fn abandon_task_without_refresh(
        &self,
        session: &SessionState,
        args: PrismFinishTaskArgs,
    ) -> Result<TaskClosureMutationResult> {
        self.close_task_without_refresh(session, args, TaskClosureDisposition::Abandoned)
    }

    fn close_task_without_refresh(
        &self,
        session: &SessionState,
        args: PrismFinishTaskArgs,
        disposition: TaskClosureDisposition,
    ) -> Result<TaskClosureMutationResult> {
        if args.summary.trim().is_empty() {
            return Err(anyhow!("task summary cannot be empty"));
        }

        let current_task = session.current_task_state();
        let task = args
            .task_id
            .map(TaskId::new)
            .or_else(|| current_task.as_ref().map(|task| task.id.clone()))
            .ok_or_else(|| anyhow!("no active task is set; provide taskId or start a task"))?;
        let metadata_override = current_task
            .as_ref()
            .filter(|state| state.id == task)
            .map(|state| (state.description.clone(), state.tags.clone()));
        let prism = self.current_prism();
        let replay = crate::load_task_replay(
            self.workspace.as_ref().map(|workspace| workspace.as_ref()),
            prism.as_ref(),
            &task,
        )
        .unwrap_or_else(|_| prism.resume_task(&task));
        if replay.events.is_empty() && metadata_override.is_none() {
            return Err(anyhow!("unknown task `{}`", task.0));
        }

        let mut anchors = replay
            .events
            .iter()
            .flat_map(|event| event.anchors.iter().cloned())
            .collect::<Vec<_>>();
        if let Some(explicit) = args.anchors {
            anchors.extend(convert_anchors(
                prism.as_ref(),
                self.workspace.as_ref().map(|workspace| workspace.root()),
                explicit,
            )?);
        }
        let anchors = prism.anchors_for(&anchors);

        let mut entry = MemoryEntry::new(MemoryKind::Episodic, args.summary.clone());
        entry.anchors = anchors.clone();
        entry.source = MemorySource::Agent;
        entry.trust = disposition.trust();
        entry.metadata = task_journal_memory_metadata(Value::Null, &task, disposition.label());
        let memory_id = session.notes.store(entry)?;
        let memory_event = session
            .notes
            .entry(&memory_id)
            .map(|entry| {
                MemoryEvent::from_entry(
                    MemoryEventKind::Stored,
                    entry,
                    Some(task.0.to_string()),
                    Vec::new(),
                    Vec::new(),
                )
            })
            .ok_or_else(|| anyhow!("stored memory `{}` could not be reloaded", memory_id.0))?;

        let event = OutcomeEvent {
            meta: EventMeta {
                id: session.next_event_id("outcome"),
                ts: current_timestamp(),
                actor: EventActor::Agent,
                correlation: Some(task.clone()),
                causation: None,
            },
            anchors,
            kind: OutcomeKind::NoteAdded,
            result: disposition.outcome_result(),
            summary: args.summary,
            evidence: Vec::new(),
            metadata: json!({
                "taskLifecycle": {
                    "disposition": disposition.label(),
                    "closed": true,
                    "memoryId": memory_id.0.clone(),
                }
            }),
        };
        let event_id = if let Some(workspace) = &self.workspace {
            let event_id =
                workspace.append_outcome_with_auxiliary(event, vec![memory_event], None, None)?;
            self.sync_workspace_revision(workspace)?;
            self.sync_episodic_revision(workspace)?;
            event_id
        } else {
            prism.apply_outcome_event_to_projections(&event);
            let id = prism.outcome_memory().store_event(event)?;
            self.persist_outcomes()?;
            self.persist_notes()?;
            id
        };

        if current_task.as_ref().is_some_and(|state| state.id == task) {
            session.clear_current_task();
        }
        self.persist_session_seed(session)?;

        let replay = crate::load_task_replay(
            self.workspace.as_ref().map(|workspace| workspace.as_ref()),
            self.current_prism().as_ref(),
            &task,
        )
        .unwrap_or_else(|_| self.current_prism().resume_task(&task));
        let journal = crate::task_journal_view_from_replay(
            session,
            self.current_prism().as_ref(),
            replay,
            metadata_override,
            DEFAULT_TASK_JOURNAL_EVENT_LIMIT,
            DEFAULT_TASK_JOURNAL_MEMORY_LIMIT,
        )?;

        Ok(TaskClosureMutationResult {
            task_id: task.0.to_string(),
            event_id: event_id.0.to_string(),
            memory_id: memory_id.0,
            journal,
        })
    }

    #[allow(dead_code)]
    pub(crate) fn store_outcome(
        &self,
        session: &SessionState,
        args: PrismOutcomeArgs,
    ) -> Result<EventMutationResult> {
        self.store_outcome_without_refresh(session, args)
    }

    pub(crate) fn store_outcome_without_refresh(
        &self,
        session: &SessionState,
        args: PrismOutcomeArgs,
    ) -> Result<EventMutationResult> {
        let prism = self.current_prism();
        let anchors = prism.anchors_for(&convert_anchors(
            prism.as_ref(),
            self.workspace.as_ref().map(|workspace| workspace.root()),
            args.anchors,
        )?);
        let task_id = session.task_for_mutation(args.task_id.map(TaskId::new));
        let event = OutcomeEvent {
            meta: EventMeta {
                id: session.next_event_id("outcome"),
                ts: current_timestamp(),
                actor: EventActor::Agent,
                correlation: Some(task_id.clone()),
                causation: None,
            },
            anchors,
            kind: convert_outcome_kind(args.kind),
            result: args
                .result
                .map(convert_outcome_result)
                .unwrap_or(prism_memory::OutcomeResult::Unknown),
            summary: args.summary,
            evidence: args
                .evidence
                .unwrap_or_default()
                .into_iter()
                .map(convert_outcome_evidence)
                .collect(),
            metadata: Value::Null,
        };
        let event_id = if let Some(workspace) = &self.workspace {
            let event_id = workspace.append_outcome(event)?;
            self.sync_workspace_revision(workspace)?;
            event_id
        } else {
            prism.apply_outcome_event_to_projections(&event);
            let id = prism.outcome_memory().store_event(event)?;
            self.persist_outcomes()?;
            id
        };
        Ok(EventMutationResult {
            event_id: event_id.0.to_string(),
            task_id: task_id.0.to_string(),
        })
    }

    #[allow(dead_code)]
    pub(crate) fn store_memory(
        &self,
        session: &SessionState,
        args: PrismMemoryArgs,
    ) -> Result<MemoryMutationResult> {
        self.store_memory_without_refresh(session, args)
    }

    pub(crate) fn store_memory_without_refresh(
        &self,
        session: &SessionState,
        args: PrismMemoryArgs,
    ) -> Result<MemoryMutationResult> {
        let task_id = session.task_for_mutation(args.task_id.map(TaskId::new));
        match args.action {
            MemoryMutationActionInput::Store => self.store_memory_payload(
                session,
                task_id,
                serde_json::from_value::<MemoryStorePayload>(args.payload)?,
            ),
            MemoryMutationActionInput::Retire => self.retire_memory_payload(
                session,
                task_id,
                serde_json::from_value::<MemoryRetirePayload>(args.payload)?,
            ),
        }
    }

    fn store_memory_payload(
        &self,
        session: &SessionState,
        task_id: TaskId,
        payload: MemoryStorePayload,
    ) -> Result<MemoryMutationResult> {
        let prism = self.current_prism();
        let anchors = prism.anchors_for(&convert_anchors(
            prism.as_ref(),
            self.workspace.as_ref().map(|workspace| workspace.root()),
            payload.anchors,
        )?);
        let kind = convert_memory_kind(payload.kind);
        let mut entry = MemoryEntry::new(kind, payload.content);
        entry.anchors = anchors;
        entry.scope = payload
            .scope
            .map(convert_memory_scope)
            .unwrap_or(MemoryScope::Session);
        entry.source = payload
            .source
            .map(convert_memory_source)
            .unwrap_or(MemorySource::Agent);
        entry.trust = payload.trust.unwrap_or(0.5).clamp(0.0, 1.0);
        entry.metadata = manual_memory_metadata(payload.metadata.unwrap_or(Value::Null), &task_id);
        if entry.scope == MemoryScope::Repo {
            entry.metadata = ensure_repo_publication_metadata(entry.metadata, current_timestamp());
        }

        let promoted_from = payload
            .promoted_from
            .clone()
            .unwrap_or_default()
            .into_iter()
            .map(prism_memory::MemoryId)
            .collect::<Vec<_>>();
        let supersedes = payload
            .supersedes
            .clone()
            .unwrap_or_default()
            .into_iter()
            .map(prism_memory::MemoryId)
            .collect::<Vec<_>>();
        ensure_repo_memory_publication_is_not_duplicate(session, &entry, supersedes.as_slice())?;

        let memory_id = session.notes.store(entry)?;
        let stored_entry = session
            .notes
            .entry(&memory_id)
            .ok_or_else(|| anyhow!("stored memory `{}` could not be reloaded", memory_id.0))?;
        if stored_entry.scope != MemoryScope::Local {
            if let Some(workspace) = &self.workspace {
                let action =
                    memory_event_kind_for_store(promoted_from.as_slice(), supersedes.as_slice());
                workspace.append_memory_event(MemoryEvent::from_entry(
                    action,
                    stored_entry.clone(),
                    Some(task_id.0.to_string()),
                    promoted_from,
                    supersedes,
                ))?;
                if stored_entry.scope == MemoryScope::Repo {
                    self.reload_episodic_snapshot(workspace)?;
                } else {
                    self.sync_episodic_revision(workspace)?;
                }
            } else if stored_entry.scope == MemoryScope::Repo {
                return Err(anyhow!(
                    "repo-published memory requires a workspace-backed PRISM session"
                ));
            }
        }
        let note_anchors = stored_entry.anchors.clone();
        let note_content = stored_entry.content.clone();
        if kind == MemoryKind::Episodic {
            if stored_entry.scope == MemoryScope::Local {
                return Ok(MemoryMutationResult {
                    memory_id: memory_id.0,
                    task_id: task_id.0.to_string(),
                });
            }
            let note_event = OutcomeEvent {
                meta: EventMeta {
                    id: session.next_event_id("outcome"),
                    ts: current_timestamp(),
                    actor: EventActor::Agent,
                    correlation: Some(task_id.clone()),
                    causation: None,
                },
                anchors: note_anchors,
                kind: prism_memory::OutcomeKind::NoteAdded,
                result: prism_memory::OutcomeResult::Success,
                summary: note_content,
                evidence: Vec::new(),
                metadata: Value::Null,
            };
            if let Some(workspace) = &self.workspace {
                let _ = workspace.append_outcome(note_event)?;
                self.sync_workspace_revision(workspace)?;
            } else {
                prism.apply_outcome_event_to_projections(&note_event);
                let _ = prism.outcome_memory().store_event(note_event)?;
                self.persist_outcomes()?;
                self.persist_notes()?;
            }
        } else if self.workspace.is_none() && stored_entry.scope != MemoryScope::Local {
            self.persist_notes()?;
        }
        Ok(MemoryMutationResult {
            memory_id: memory_id.0,
            task_id: task_id.0.to_string(),
        })
    }

    fn retire_memory_payload(
        &self,
        session: &SessionState,
        task_id: TaskId,
        payload: MemoryRetirePayload,
    ) -> Result<MemoryMutationResult> {
        let workspace = self.workspace.as_ref().ok_or_else(|| {
            anyhow!("retiring repo-published memory requires a workspace-backed session")
        })?;
        let memory_id = prism_memory::MemoryId(payload.memory_id.clone());
        let existing = session
            .notes
            .entry(&memory_id)
            .ok_or_else(|| anyhow!("unknown memory `{}`", payload.memory_id))?;
        if existing.scope != MemoryScope::Repo {
            return Err(anyhow!(
                "only repo-published memory can be retired through prism_mutate"
            ));
        }
        let already_retired = existing
            .metadata
            .get("publication")
            .and_then(|value| value.get("status"))
            .and_then(Value::as_str)
            .is_some_and(|status| status.eq_ignore_ascii_case("retired"));
        if already_retired {
            return Err(anyhow!("memory `{}` is already retired", payload.memory_id));
        }

        let mut retired_entry = existing;
        retired_entry.metadata = retire_repo_publication_metadata(
            retired_entry.metadata,
            current_timestamp(),
            &payload.retirement_reason,
        );
        workspace.append_memory_event(MemoryEvent::from_entry(
            MemoryEventKind::Retired,
            retired_entry,
            Some(task_id.0.to_string()),
            Vec::new(),
            Vec::new(),
        ))?;
        self.reload_episodic_snapshot(workspace)?;
        Ok(MemoryMutationResult {
            memory_id: payload.memory_id,
            task_id: task_id.0.to_string(),
        })
    }

    #[allow(dead_code)]
    pub(crate) fn store_concept(
        &self,
        session: &SessionState,
        args: PrismConceptMutationArgs,
    ) -> Result<ConceptMutationResult> {
        self.store_concept_without_refresh(session, args)
    }

    pub(crate) fn store_concept_without_refresh(
        &self,
        session: &SessionState,
        args: PrismConceptMutationArgs,
    ) -> Result<ConceptMutationResult> {
        let workspace = self.workspace.as_ref().ok_or_else(|| {
            anyhow!("concept promotion requires a workspace-backed PRISM session")
        })?;
        let prism = self.current_prism();
        let task_id = session.task_for_mutation(args.task_id.clone().map(TaskId::new));
        let operation = args.operation.clone();
        let recorded_at = current_timestamp();
        let packet = match operation {
            ConceptMutationOperationInput::Promote => {
                build_promoted_concept_packet(prism.as_ref(), &task_id, recorded_at, args.clone())?
            }
            ConceptMutationOperationInput::Update => {
                build_updated_concept_packet(prism.as_ref(), &task_id, recorded_at, args.clone())?
            }
            ConceptMutationOperationInput::Retire => {
                build_retired_concept_packet(prism.as_ref(), &task_id, recorded_at, args.clone())?
            }
        };
        let patch = concept_event_patch(&args, &operation, &packet)?;
        let event = ConceptEvent {
            id: next_concept_event_id(),
            recorded_at,
            task_id: Some(task_id.0.to_string()),
            action: match operation {
                ConceptMutationOperationInput::Promote => ConceptEventAction::Promote,
                ConceptMutationOperationInput::Update => ConceptEventAction::Update,
                ConceptMutationOperationInput::Retire => ConceptEventAction::Retire,
            },
            patch,
            concept: packet.clone(),
        };
        workspace.append_concept_event(event.clone())?;
        self.sync_workspace_revision(workspace)?;
        Ok(ConceptMutationResult {
            event_id: event.id,
            concept_handle: packet.handle.clone(),
            task_id: task_id.0.to_string(),
            packet: concept_packet_view(prism.as_ref(), packet, ConceptVerbosity::Full, true, None),
        })
    }

    #[allow(dead_code)]
    pub(crate) fn store_contract(
        &self,
        session: &SessionState,
        args: PrismContractMutationArgs,
    ) -> Result<ContractMutationResult> {
        self.store_contract_without_refresh(session, args)
    }

    pub(crate) fn store_contract_without_refresh(
        &self,
        session: &SessionState,
        args: PrismContractMutationArgs,
    ) -> Result<ContractMutationResult> {
        let workspace = self.workspace.as_ref().ok_or_else(|| {
            anyhow!("contract mutations require a workspace-backed PRISM session")
        })?;
        let prism = self.current_prism();
        let task_id = session.task_for_mutation(args.task_id.clone().map(TaskId::new));
        let operation = args.operation.clone();
        let recorded_at = current_timestamp();
        let workspace_root = Some(workspace.root());
        let packet = match operation {
            ContractMutationOperationInput::Promote => build_promoted_contract_packet(
                prism.as_ref(),
                workspace_root,
                &task_id,
                recorded_at,
                args.clone(),
            )?,
            ContractMutationOperationInput::Update => build_updated_contract_packet(
                prism.as_ref(),
                workspace_root,
                &task_id,
                recorded_at,
                args.clone(),
            )?,
            ContractMutationOperationInput::Retire => {
                build_retired_contract_packet(prism.as_ref(), &task_id, recorded_at, args.clone())?
            }
            ContractMutationOperationInput::AttachEvidence => {
                build_contract_with_evidence_attached(
                    prism.as_ref(),
                    &task_id,
                    recorded_at,
                    args.clone(),
                )?
            }
            ContractMutationOperationInput::AttachValidation => {
                build_contract_with_validation_attached(
                    prism.as_ref(),
                    workspace_root,
                    &task_id,
                    recorded_at,
                    args.clone(),
                )?
            }
            ContractMutationOperationInput::RecordConsumer => {
                build_contract_with_consumer_recorded(
                    prism.as_ref(),
                    workspace_root,
                    &task_id,
                    recorded_at,
                    args.clone(),
                )?
            }
            ContractMutationOperationInput::SetStatus => {
                build_contract_with_status_set(prism.as_ref(), &task_id, recorded_at, args.clone())?
            }
        };
        let patch = contract_event_patch(&args, &operation, &packet)?;
        let event = ContractEvent {
            id: next_contract_event_id(),
            recorded_at,
            task_id: Some(task_id.0.to_string()),
            action: match operation {
                ContractMutationOperationInput::Promote => ContractEventAction::Promote,
                ContractMutationOperationInput::Update => ContractEventAction::Update,
                ContractMutationOperationInput::Retire => ContractEventAction::Retire,
                ContractMutationOperationInput::AttachEvidence => {
                    ContractEventAction::AttachEvidence
                }
                ContractMutationOperationInput::AttachValidation => {
                    ContractEventAction::AttachValidation
                }
                ContractMutationOperationInput::RecordConsumer => {
                    ContractEventAction::RecordConsumer
                }
                ContractMutationOperationInput::SetStatus => ContractEventAction::SetStatus,
            },
            patch,
            contract: packet.clone(),
        };
        workspace.append_contract_event(event.clone())?;
        self.sync_workspace_revision(workspace)?;
        Ok(ContractMutationResult {
            event_id: event.id,
            contract_handle: packet.handle.clone(),
            task_id: task_id.0.to_string(),
            packet: contract_packet_view(
                prism.as_ref(),
                self.workspace.as_ref().map(|workspace| workspace.root()),
                packet,
                None,
            ),
        })
    }

    pub(crate) fn store_concept_relation(
        &self,
        session: &SessionState,
        args: PrismConceptRelationMutationArgs,
    ) -> Result<ConceptRelationMutationResult> {
        let workspace = self.workspace.as_ref().ok_or_else(|| {
            anyhow!("concept relation mutations require a workspace-backed PRISM session")
        })?;
        let prism = self.current_prism();
        let task_id = session.task_for_mutation(args.task_id.clone().map(TaskId::new));
        let relation = build_concept_relation(prism.as_ref(), &task_id, &args)?;
        let event = ConceptRelationEvent {
            id: next_concept_relation_event_id(),
            recorded_at: current_timestamp(),
            task_id: Some(task_id.0.to_string()),
            action: match args.operation {
                ConceptRelationMutationOperationInput::Upsert => ConceptRelationEventAction::Upsert,
                ConceptRelationMutationOperationInput::Retire => ConceptRelationEventAction::Retire,
            },
            relation: relation.clone(),
        };
        workspace.append_concept_relation_event(event.clone())?;
        self.sync_workspace_revision(workspace)?;
        let focus_handle = relation.source_handle.clone();
        Ok(ConceptRelationMutationResult {
            event_id: event.id,
            task_id: task_id.0.to_string(),
            relation: concept_relation_view(prism.as_ref(), &focus_handle, relation),
        })
    }

    #[allow(dead_code)]
    pub(crate) fn store_validation_feedback(
        &self,
        session: &SessionState,
        args: PrismValidationFeedbackArgs,
    ) -> Result<ValidationFeedbackMutationResult> {
        self.store_validation_feedback_without_refresh(session, args)
    }

    pub(crate) fn store_validation_feedback_without_refresh(
        &self,
        session: &SessionState,
        args: PrismValidationFeedbackArgs,
    ) -> Result<ValidationFeedbackMutationResult> {
        let prism = self.current_prism();
        let task_id = session.task_for_mutation(args.task_id.map(TaskId::new));
        let anchors = prism.anchors_for(&convert_anchors(
            prism.as_ref(),
            self.workspace.as_ref().map(|workspace| workspace.root()),
            args.anchors.unwrap_or_default(),
        )?);
        let workspace = self.workspace.as_ref().ok_or_else(|| {
            anyhow!("validation feedback logging requires a workspace-backed PRISM session")
        })?;
        let entry = workspace.append_validation_feedback(ValidationFeedbackRecord {
            task_id: Some(task_id.0.to_string()),
            context: args.context,
            anchors,
            prism_said: args.prism_said,
            actually_true: args.actually_true,
            category: convert_validation_feedback_category(args.category),
            verdict: convert_validation_feedback_verdict(args.verdict),
            corrected_manually: args.corrected_manually.unwrap_or(false),
            correction: args.correction,
            metadata: args.metadata.unwrap_or(Value::Null),
        })?;
        Ok(ValidationFeedbackMutationResult {
            entry_id: entry.id,
            task_id: task_id.0.to_string(),
        })
    }

    pub(crate) fn store_inferred_edge(
        &self,
        session: &SessionState,
        args: PrismInferEdgeArgs,
    ) -> Result<EdgeMutationResult> {
        let task = session.task_for_mutation(args.task_id.map(TaskId::new));
        let edge = Edge {
            kind: parse_edge_kind(&args.kind)?,
            source: convert_node_id(args.source)?,
            target: convert_node_id(args.target)?,
            origin: EdgeOrigin::Inferred,
            confidence: args.confidence.clamp(0.0, 1.0),
        };
        let scope = args
            .scope
            .map(convert_inferred_scope)
            .unwrap_or(prism_agent::InferredEdgeScope::SessionOnly);
        let id = session.inferred_edges.store_edge(
            edge,
            scope,
            Some(task.clone()),
            args.evidence.unwrap_or_default(),
        );
        if scope != prism_agent::InferredEdgeScope::SessionOnly {
            if let Some(workspace) = &self.workspace {
                let record = session.inferred_edges.record(&id).ok_or_else(|| {
                    anyhow!("stored inferred edge `{}` could not be reloaded", id.0)
                })?;
                workspace.append_inference_records(&[record])?;
                self.sync_inference_revision(workspace)?;
            } else {
                self.persist_inferred_edges()?;
            }
        }
        Ok(EdgeMutationResult {
            edge_id: id.0,
            task_id: task.0.to_string(),
        })
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn store_coordination(
        &self,
        session: &SessionState,
        args: PrismCoordinationArgs,
    ) -> Result<CoordinationMutationResult> {
        self.store_coordination_with_trace(session, args, None)
    }

    pub(crate) fn store_coordination_traced(
        &self,
        session: &SessionState,
        args: PrismCoordinationArgs,
        trace: &MutationRun,
    ) -> Result<CoordinationMutationResult> {
        self.store_coordination_with_trace(session, args, Some(trace))
    }

    fn store_coordination_with_trace(
        &self,
        session: &SessionState,
        args: PrismCoordinationArgs,
        trace: Option<&MutationRun>,
    ) -> Result<CoordinationMutationResult> {
        self.ensure_tool_enabled("prism_coordination", "coordination workflow mutations")?;
        if let Some(workspace) = &self.workspace {
            let refresh_started = std::time::Instant::now();
            match self.refresh_workspace_for_mutation() {
                Ok(report) => {
                    if let Some(trace) = trace {
                        trace.record_phase(
                            "mutation.coordination.refreshWorkspace",
                            &json!({
                                "refreshPath": report.refresh_path,
                                "deferred": report.deferred,
                                "episodicReloaded": report.episodic_reloaded,
                                "inferenceReloaded": report.inference_reloaded,
                                "coordinationReloaded": report.coordination_reloaded,
                                "metrics": report.metrics.as_json(),
                            }),
                            refresh_started.elapsed(),
                            true,
                            None,
                        );
                    }
                }
                Err(error) => {
                    if let Some(trace) = trace {
                        trace.record_phase(
                            "mutation.coordination.refreshWorkspace",
                            &json!({ "refreshPath": "error" }),
                            refresh_started.elapsed(),
                            false,
                            Some(error.to_string()),
                        );
                    }
                    return Err(error);
                }
            }
            let sync_started = std::time::Instant::now();
            match self.sync_coordination_revision(workspace) {
                Ok(()) => {
                    if let Some(trace) = trace {
                        trace.record_phase(
                            "mutation.coordination.syncLoadedRevisionBefore",
                            &json!({
                                "loadedRevision": self
                                    .loaded_coordination_revision
                                    .load(Ordering::Relaxed),
                            }),
                            sync_started.elapsed(),
                            true,
                            None,
                        );
                    }
                }
                Err(error) => {
                    if let Some(trace) = trace {
                        trace.record_phase(
                            "mutation.coordination.syncLoadedRevisionBefore",
                            &json!({}),
                            sync_started.elapsed(),
                            false,
                            Some(error.to_string()),
                        );
                    }
                    return Err(error);
                }
            }
        }
        let prism = self.current_prism();
        let before_events = prism.coordination_events().len();
        let task = session.task_for_mutation(args.task_id.clone().map(TaskId::new));
        let event_id = session.next_event_id("coordination");
        let meta = EventMeta {
            id: event_id.clone(),
            ts: current_timestamp(),
            actor: EventActor::Agent,
            correlation: Some(task),
            causation: None,
        };
        if let Some(workspace) = &self.workspace {
            let result = if let Some(trace) = trace {
                workspace.mutate_coordination_with_session_observed(
                    Some(&session.session_id()),
                    |prism| self.apply_coordination_mutation(session, prism, args, meta.clone()),
                    |operation, duration, args, success, error| {
                        trace.record_phase(operation, &args, duration, success, error)
                    },
                )
            } else {
                workspace.mutate_coordination_with_session(Some(&session.session_id()), |prism| {
                    self.apply_coordination_mutation(session, prism, args, meta.clone())
                })
            };
            match result {
                Ok(state) => {
                    let sync_started = std::time::Instant::now();
                    match self.sync_coordination_revision(workspace) {
                        Ok(()) => {
                            if let Some(trace) = trace {
                                trace.record_phase(
                                    "mutation.coordination.syncLoadedRevisionAfter",
                                    &json!({
                                        "loadedRevision": self
                                            .loaded_coordination_revision
                                            .load(Ordering::Relaxed),
                                    }),
                                    sync_started.elapsed(),
                                    true,
                                    None,
                                );
                            }
                        }
                        Err(error) => {
                            if let Some(trace) = trace {
                                trace.record_phase(
                                    "mutation.coordination.syncLoadedRevisionAfter",
                                    &json!({}),
                                    sync_started.elapsed(),
                                    false,
                                    Some(error.to_string()),
                                );
                            }
                            return Err(error);
                        }
                    }
                    let prism = self.current_prism();
                    let audit = coordination_audit_since(prism.as_ref(), before_events);
                    return Ok(CoordinationMutationResult {
                        event_id: event_id.0.to_string(),
                        event_ids: audit.event_ids,
                        rejected: false,
                        violations: audit.violations,
                        state,
                    });
                }
                Err(error) => {
                    let prism = self.current_prism();
                    let audit = coordination_audit_since(prism.as_ref(), before_events);
                    if audit.rejected && !audit.event_ids.is_empty() {
                        let sync_started = std::time::Instant::now();
                        match self.sync_coordination_revision(workspace) {
                            Ok(()) => {
                                if let Some(trace) = trace {
                                    trace.record_phase(
                                        "mutation.coordination.syncLoadedRevisionAfter",
                                        &json!({
                                            "loadedRevision": self
                                                .loaded_coordination_revision
                                                .load(Ordering::Relaxed),
                                        }),
                                        sync_started.elapsed(),
                                        true,
                                        None,
                                    );
                                }
                            }
                            Err(sync_error) => {
                                if let Some(trace) = trace {
                                    trace.record_phase(
                                        "mutation.coordination.syncLoadedRevisionAfter",
                                        &json!({}),
                                        sync_started.elapsed(),
                                        false,
                                        Some(sync_error.to_string()),
                                    );
                                }
                                return Err(sync_error);
                            }
                        }
                        return Ok(CoordinationMutationResult {
                            event_id: audit
                                .event_ids
                                .first()
                                .cloned()
                                .unwrap_or_else(|| event_id.0.to_string()),
                            event_ids: audit.event_ids,
                            rejected: true,
                            violations: audit.violations,
                            state: Value::Null,
                        });
                    }
                    return Err(error);
                }
            }
        }
        let state =
            match self.apply_coordination_mutation(session, prism.as_ref(), args, meta.clone()) {
                Ok(state) => state,
                Err(error) => {
                    let audit = coordination_audit_since(prism.as_ref(), before_events);
                    if audit.rejected && !audit.event_ids.is_empty() {
                        return Ok(CoordinationMutationResult {
                            event_id: audit
                                .event_ids
                                .first()
                                .cloned()
                                .unwrap_or_else(|| event_id.0.to_string()),
                            event_ids: audit.event_ids,
                            rejected: true,
                            violations: audit.violations,
                            state: Value::Null,
                        });
                    }
                    return Err(error);
                }
            };
        let audit = coordination_audit_since(prism.as_ref(), before_events);
        Ok(CoordinationMutationResult {
            event_id: event_id.0.to_string(),
            event_ids: audit.event_ids,
            rejected: false,
            violations: audit.violations,
            state,
        })
    }

    pub(crate) fn store_claim(
        &self,
        session: &SessionState,
        args: PrismClaimArgs,
    ) -> Result<ClaimMutationResult> {
        self.ensure_tool_enabled("prism_claim", "coordination claim mutations")?;
        if let Some(workspace) = &self.workspace {
            self.refresh_workspace_for_mutation()?;
            self.sync_coordination_revision(workspace)?;
        }
        let prism = self.current_prism();
        let before_events = prism.coordination_events().len();
        let task = session.task_for_mutation(args.task_id.clone().map(TaskId::new));
        let meta = EventMeta {
            id: session.next_event_id("coordination"),
            ts: current_timestamp(),
            actor: EventActor::Agent,
            correlation: Some(task),
            causation: None,
        };
        if let Some(workspace) = &self.workspace {
            match workspace.mutate_coordination_with_session(Some(&session.session_id()), |prism| {
                self.apply_claim_mutation(session, prism, args, meta.clone())
            }) {
                Ok(mut result) => {
                    self.sync_coordination_revision(workspace)?;
                    let prism = self.current_prism();
                    let audit = coordination_audit_since(prism.as_ref(), before_events);
                    result.event_ids = audit.event_ids;
                    result.violations.extend(audit.violations);
                    Ok(result)
                }
                Err(error) => {
                    let prism = self.current_prism();
                    let audit = coordination_audit_since(prism.as_ref(), before_events);
                    if audit.rejected && !audit.event_ids.is_empty() {
                        self.sync_coordination_revision(workspace)?;
                        return Ok(ClaimMutationResult {
                            claim_id: None,
                            event_ids: audit.event_ids,
                            rejected: true,
                            conflicts: Vec::new(),
                            violations: audit.violations,
                            state: Value::Null,
                        });
                    }
                    Err(error)
                }
            }
        } else {
            match self.apply_claim_mutation(session, prism.as_ref(), args, meta.clone()) {
                Ok(mut result) => {
                    let audit = coordination_audit_since(prism.as_ref(), before_events);
                    result.event_ids = audit.event_ids;
                    result.violations.extend(audit.violations);
                    Ok(result)
                }
                Err(error) => {
                    let audit = coordination_audit_since(prism.as_ref(), before_events);
                    if audit.rejected && !audit.event_ids.is_empty() {
                        return Ok(ClaimMutationResult {
                            claim_id: None,
                            event_ids: audit.event_ids,
                            rejected: true,
                            conflicts: Vec::new(),
                            violations: audit.violations,
                            state: Value::Null,
                        });
                    }
                    Err(error)
                }
            }
        }
    }

    pub(crate) fn store_artifact(
        &self,
        session: &SessionState,
        args: PrismArtifactArgs,
    ) -> Result<ArtifactMutationResult> {
        self.ensure_tool_enabled("prism_artifact", "coordination artifact mutations")?;
        if let Some(workspace) = &self.workspace {
            self.refresh_workspace_for_mutation()?;
            self.sync_coordination_revision(workspace)?;
        }
        let prism = self.current_prism();
        let before_events = prism.coordination_events().len();
        let task = session.task_for_mutation(args.task_id.clone().map(TaskId::new));
        let meta = EventMeta {
            id: session.next_event_id("coordination"),
            ts: current_timestamp(),
            actor: EventActor::Agent,
            correlation: Some(task),
            causation: None,
        };
        if let Some(workspace) = &self.workspace {
            match workspace.mutate_coordination_with_session(Some(&session.session_id()), |prism| {
                self.apply_artifact_mutation(prism, args, meta.clone())
            }) {
                Ok(mut result) => {
                    self.sync_coordination_revision(workspace)?;
                    let prism = self.current_prism();
                    let audit = coordination_audit_since(prism.as_ref(), before_events);
                    result.event_ids = audit.event_ids;
                    result.violations.extend(audit.violations);
                    Ok(result)
                }
                Err(error) => {
                    let prism = self.current_prism();
                    let audit = coordination_audit_since(prism.as_ref(), before_events);
                    if audit.rejected && !audit.event_ids.is_empty() {
                        self.sync_coordination_revision(workspace)?;
                        return Ok(ArtifactMutationResult {
                            artifact_id: None,
                            review_id: None,
                            event_ids: audit.event_ids,
                            rejected: true,
                            violations: audit.violations,
                            state: Value::Null,
                        });
                    }
                    Err(error)
                }
            }
        } else {
            match self.apply_artifact_mutation(prism.as_ref(), args, meta.clone()) {
                Ok(mut result) => {
                    let audit = coordination_audit_since(prism.as_ref(), before_events);
                    result.event_ids = audit.event_ids;
                    result.violations.extend(audit.violations);
                    Ok(result)
                }
                Err(error) => {
                    let audit = coordination_audit_since(prism.as_ref(), before_events);
                    if audit.rejected && !audit.event_ids.is_empty() {
                        return Ok(ArtifactMutationResult {
                            artifact_id: None,
                            review_id: None,
                            event_ids: audit.event_ids,
                            rejected: true,
                            violations: audit.violations,
                            state: Value::Null,
                        });
                    }
                    Err(error)
                }
            }
        }
    }

    pub(crate) fn apply_coordination_mutation(
        &self,
        session: &SessionState,
        prism: &Prism,
        args: PrismCoordinationArgs,
        meta: EventMeta,
    ) -> Result<Value> {
        let workspace_root = self.workspace.as_ref().map(|workspace| workspace.root());
        match args.kind {
            CoordinationMutationKindInput::PlanCreate => {
                let payload: crate::PlanCreatePayload = serde_json::from_value(args.payload)?;
                let plan_id = prism.create_native_plan(
                    meta,
                    payload.goal,
                    payload.status.map(convert_plan_status),
                    convert_policy(payload.policy)?,
                )?;
                let plan = prism
                    .coordination_plan(&plan_id)
                    .ok_or_else(|| anyhow!("unknown plan `{}`", plan_id.0))?;
                let root_node_ids = prism
                    .plan_graph(&plan_id)
                    .map(|graph| graph.root_nodes)
                    .unwrap_or_else(|| {
                        plan.root_tasks
                            .iter()
                            .map(|task_id| prism_ir::PlanNodeId::new(task_id.0.clone()))
                            .collect()
                    });
                Ok(serde_json::to_value(plan_view(plan, root_node_ids))?)
            }
            CoordinationMutationKindInput::PlanUpdate => {
                let payload: PlanUpdatePayload = serde_json::from_value(args.payload)?;
                let plan_id = PlanId::new(payload.plan_id);
                prism.update_native_plan(
                    meta,
                    &plan_id,
                    payload.status.map(convert_plan_status),
                    payload.goal,
                    convert_policy(payload.policy)?,
                )?;
                let plan = prism
                    .coordination_plan(&plan_id)
                    .ok_or_else(|| anyhow!("unknown plan `{}`", plan_id.0))?;
                let root_node_ids = prism
                    .plan_graph(&plan_id)
                    .map(|graph| graph.root_nodes)
                    .unwrap_or_else(|| {
                        plan.root_tasks
                            .iter()
                            .map(|task_id| prism_ir::PlanNodeId::new(task_id.0.clone()))
                            .collect()
                    });
                Ok(serde_json::to_value(plan_view(plan, root_node_ids))?)
            }
            CoordinationMutationKindInput::TaskCreate => {
                let payload: TaskCreatePayload = serde_json::from_value(args.payload)?;
                let task = prism.create_native_task(
                    meta,
                    TaskCreateInput {
                        plan_id: PlanId::new(payload.plan_id),
                        title: payload.title,
                        status: payload.status.map(convert_coordination_task_status),
                        assignee: payload
                            .assignee
                            .map(AgentId::new)
                            .or_else(|| session.current_agent()),
                        session: Some(session.session_id()),
                        worktree_id: None,
                        branch_ref: None,
                        anchors: convert_anchors(
                            prism,
                            workspace_root,
                            payload.anchors.unwrap_or_default(),
                        )?,
                        depends_on: payload
                            .depends_on
                            .unwrap_or_default()
                            .into_iter()
                            .map(CoordinationTaskId::new)
                            .collect(),
                        acceptance: convert_acceptance(prism, workspace_root, payload.acceptance)?,
                        base_revision: prism.workspace_revision(),
                    },
                )?;
                Ok(serde_json::to_value(coordination_task_view(task))?)
            }
            CoordinationMutationKindInput::Update => {
                let payload: WorkflowUpdatePayload = serde_json::from_value(args.payload)?;
                let WorkflowUpdatePayload {
                    id,
                    kind,
                    status,
                    assignee,
                    is_abstract,
                    title,
                    summary,
                    anchors,
                    bindings,
                    depends_on,
                    acceptance,
                    validation_refs,
                    priority,
                    tags,
                    completion_context,
                } = payload;
                let prefer_plan_node = kind.is_some()
                    || is_abstract.is_some()
                    || bindings.is_some()
                    || validation_refs.is_some()
                    || tags.is_some()
                    || !matches!(
                        parse_sparse_patch(summary.clone(), "summary")?,
                        SparsePatch::Keep
                    )
                    || !matches!(
                        parse_sparse_patch(priority.clone(), "priority")?,
                        SparsePatch::Keep
                    );
                match resolve_workflow_update_target_with_preference(prism, &id, prefer_plan_node)?
                {
                    WorkflowUpdateTarget::CoordinationTask(task_id) => {
                        let summary_patch = parse_sparse_patch(summary, "summary")?;
                        if !matches!(summary_patch, SparsePatch::Keep) {
                            return Err(anyhow!(
                                "field `summary` is only supported when `id` resolves to a native plan node"
                            ));
                        }
                        let priority_patch = parse_sparse_patch(priority, "priority")?;
                        if !matches!(priority_patch, SparsePatch::Keep) {
                            return Err(anyhow!(
                                "field `priority` is only supported when `id` resolves to a native plan node"
                            ));
                        }
                        if kind.is_some()
                            || is_abstract.is_some()
                            || bindings.is_some()
                            || validation_refs.is_some()
                            || tags.is_some()
                        {
                            return Err(anyhow!(
                                "fields `kind`, `isAbstract`, `bindings`, `validationRefs`, and `tags` are only supported when `id` resolves to a native plan node"
                            ));
                        }
                        let status = status.map(convert_workflow_status_for_task).transpose()?;
                        let assignee = match parse_sparse_patch(assignee, "assignee")? {
                            SparsePatch::Keep => None,
                            SparsePatch::Set(value) => Some(Some(AgentId::new(value))),
                            SparsePatch::Clear => Some(None),
                        };
                        let completion_context = convert_completion_context(completion_context)
                            .or_else(|| {
                                status
                                    .filter(|status| {
                                        *status == prism_ir::CoordinationTaskStatus::Completed
                                    })
                                    .and_then(|_| prism.task_risk(&task_id, meta.ts))
                                    .map(|risk| prism_coordination::TaskCompletionContext {
                                        risk_score: Some(risk.risk_score),
                                        required_validations: risk.likely_validations,
                                    })
                            });
                        let task = prism.update_native_task(
                            meta,
                            TaskUpdateInput {
                                task_id,
                                status,
                                assignee,
                                session: None,
                                worktree_id: None,
                                branch_ref: None,
                                title,
                                anchors: anchors
                                    .map(|anchors| convert_anchors(prism, workspace_root, anchors))
                                    .transpose()?,
                                depends_on: depends_on.map(|depends_on| {
                                    depends_on
                                        .into_iter()
                                        .map(CoordinationTaskId::new)
                                        .collect::<Vec<_>>()
                                }),
                                acceptance: acceptance
                                    .map(|acceptance| {
                                        convert_acceptance(prism, workspace_root, Some(acceptance))
                                    })
                                    .transpose()?,
                                base_revision: Some(prism.workspace_revision()),
                                completion_context,
                            },
                            prism.workspace_revision(),
                            current_timestamp(),
                        )?;
                        Ok(serde_json::to_value(coordination_task_view(task))?)
                    }
                    WorkflowUpdateTarget::PlanNode { plan_id, node_id } => {
                        let status = status.map(convert_workflow_status_for_plan_node);
                        let assignee = match parse_sparse_patch(assignee, "assignee")? {
                            SparsePatch::Keep => None,
                            SparsePatch::Set(value) => Some(Some(AgentId::new(value))),
                            SparsePatch::Clear => Some(None),
                        };
                        let (summary, clear_summary) = match parse_sparse_patch(summary, "summary")?
                        {
                            SparsePatch::Keep => (None, false),
                            SparsePatch::Set(value) => (Some(value), false),
                            SparsePatch::Clear => (None, true),
                        };
                        let (priority, clear_priority) =
                            match parse_sparse_patch(priority, "priority")? {
                                SparsePatch::Keep => (None, false),
                                SparsePatch::Set(value) => (Some(value), false),
                                SparsePatch::Clear => (None, true),
                            };
                        prism.update_native_plan_node(
                            &node_id,
                            kind.map(convert_plan_node_kind),
                            status,
                            assignee,
                            is_abstract,
                            title,
                            summary,
                            clear_summary,
                            convert_plan_binding(prism, workspace_root, anchors, bindings)?,
                            depends_on,
                            acceptance
                                .map(|acceptance| {
                                    convert_plan_acceptance(prism, workspace_root, Some(acceptance))
                                })
                                .transpose()?,
                            validation_refs.map(|refs| convert_validation_refs(Some(refs))),
                            Some(prism.workspace_revision()),
                            priority,
                            clear_priority,
                            tags,
                        )?;
                        current_plan_node_state(prism, &plan_id, &node_id.0)
                    }
                }
            }
            CoordinationMutationKindInput::PlanNodeCreate => {
                let payload: PlanNodeCreatePayload = serde_json::from_value(args.payload)?;
                let kind = payload
                    .kind
                    .map(convert_plan_node_kind)
                    .unwrap_or(prism_ir::PlanNodeKind::Edit);
                let status = payload.status.map(convert_plan_node_status);
                let plan_id = PlanId::new(payload.plan_id.clone());
                let node_id = prism.create_native_plan_node(
                    &plan_id,
                    kind,
                    payload.title,
                    payload.summary,
                    status,
                    payload
                        .assignee
                        .map(AgentId::new)
                        .or_else(|| session.current_agent()),
                    payload.is_abstract.unwrap_or(false),
                    convert_plan_binding(prism, workspace_root, payload.anchors, payload.bindings)?
                        .unwrap_or_default(),
                    payload.depends_on.unwrap_or_default(),
                    convert_plan_acceptance(prism, workspace_root, payload.acceptance)?,
                    convert_validation_refs(payload.validation_refs),
                    prism.workspace_revision(),
                    payload.priority,
                    payload.tags.unwrap_or_default(),
                )?;
                current_plan_node_state(prism, &plan_id, &node_id.0)
            }
            CoordinationMutationKindInput::PlanEdgeCreate => {
                let payload: PlanEdgeCreatePayload = serde_json::from_value(args.payload)?;
                let kind = convert_plan_edge_kind(payload.kind);
                let plan_id = PlanId::new(payload.plan_id.clone());
                prism.create_native_plan_edge(
                    &plan_id,
                    &PlanNodeId::new(payload.from_node_id.clone()),
                    &PlanNodeId::new(payload.to_node_id.clone()),
                    kind,
                )?;
                current_plan_edge_state(
                    prism,
                    &plan_id,
                    &payload.from_node_id,
                    &payload.to_node_id,
                    kind,
                )
            }
            CoordinationMutationKindInput::PlanEdgeDelete => {
                let payload: PlanEdgeDeletePayload = serde_json::from_value(args.payload)?;
                let kind = convert_plan_edge_kind(payload.kind);
                let plan_id = PlanId::new(payload.plan_id.clone());
                prism.delete_native_plan_edge(
                    &plan_id,
                    &PlanNodeId::new(payload.from_node_id.clone()),
                    &PlanNodeId::new(payload.to_node_id.clone()),
                    kind,
                )?;
                deleted_plan_edge_state(&plan_id, &payload.from_node_id, &payload.to_node_id, kind)
            }
            CoordinationMutationKindInput::Handoff => {
                let payload: crate::HandoffPayload = serde_json::from_value(args.payload)?;
                let task = prism.request_native_handoff(
                    meta,
                    HandoffInput {
                        task_id: CoordinationTaskId::new(payload.task_id),
                        to_agent: payload.to_agent.map(AgentId::new),
                        summary: payload.summary,
                        base_revision: prism.workspace_revision(),
                    },
                    prism.workspace_revision(),
                )?;
                Ok(serde_json::to_value(coordination_task_view(task))?)
            }
            CoordinationMutationKindInput::HandoffAccept => {
                let payload: HandoffAcceptPayload = serde_json::from_value(args.payload)?;
                let session_agent = session.current_agent();
                if let (Some(expected), Some(current)) =
                    (payload.agent.as_ref(), session_agent.as_ref())
                {
                    if expected != &current.0 {
                        return Err(anyhow!(
                            "handoff acceptance agent `{expected}` does not match current session agent `{}`",
                            current.0
                        ));
                    }
                }
                let task = prism.accept_native_handoff(
                    meta,
                    HandoffAcceptInput {
                        task_id: CoordinationTaskId::new(payload.task_id),
                        agent: session_agent,
                        worktree_id: None,
                        branch_ref: None,
                    },
                )?;
                Ok(serde_json::to_value(coordination_task_view(task))?)
            }
        }
    }

    pub(crate) fn apply_claim_mutation(
        &self,
        session: &SessionState,
        prism: &Prism,
        args: PrismClaimArgs,
        meta: EventMeta,
    ) -> Result<ClaimMutationResult> {
        let workspace_root = self.workspace.as_ref().map(|workspace| workspace.root());
        match args.action {
            ClaimActionInput::Acquire => {
                let payload: ClaimAcquirePayload = serde_json::from_value(args.payload)?;
                let anchors = prism.coordination_scope_anchors(&convert_anchors(
                    prism,
                    workspace_root,
                    payload.anchors,
                )?);
                let (claim_id, conflicts, state) = prism.acquire_native_claim(
                    meta,
                    session.session_id(),
                    prism_coordination::ClaimAcquireInput {
                        task_id: payload.coordination_task_id.map(CoordinationTaskId::new),
                        anchors,
                        capability: convert_capability(payload.capability),
                        mode: payload.mode.map(convert_claim_mode),
                        ttl_seconds: payload.ttl_seconds,
                        base_revision: prism.workspace_revision(),
                        current_revision: prism.workspace_revision(),
                        agent: payload
                            .agent
                            .map(AgentId::new)
                            .or_else(|| session.current_agent()),
                        worktree_id: None,
                        branch_ref: None,
                    },
                )?;
                Ok(ClaimMutationResult {
                    claim_id: claim_id.map(|claim_id| claim_id.0.to_string()),
                    event_ids: Vec::new(),
                    rejected: false,
                    conflicts: conflicts
                        .into_iter()
                        .map(conflict_view)
                        .map(serde_json::to_value)
                        .collect::<Result<Vec<_>, _>>()?,
                    violations: Vec::new(),
                    state: state
                        .map(claim_view)
                        .map(serde_json::to_value)
                        .transpose()?
                        .unwrap_or(Value::Null),
                })
            }
            ClaimActionInput::Renew => {
                let payload: ClaimRenewPayload = serde_json::from_value(args.payload)?;
                let claim = prism.renew_native_claim(
                    meta,
                    &session.session_id(),
                    &ClaimId::new(payload.claim_id.clone()),
                    payload.ttl_seconds,
                )?;
                Ok(ClaimMutationResult {
                    claim_id: Some(payload.claim_id),
                    event_ids: Vec::new(),
                    rejected: false,
                    conflicts: Vec::new(),
                    violations: Vec::new(),
                    state: serde_json::to_value(claim_view(claim))?,
                })
            }
            ClaimActionInput::Release => {
                let payload: ClaimReleasePayload = serde_json::from_value(args.payload)?;
                let claim = prism.release_native_claim(
                    meta,
                    &session.session_id(),
                    &ClaimId::new(payload.claim_id.clone()),
                )?;
                Ok(ClaimMutationResult {
                    claim_id: Some(payload.claim_id),
                    event_ids: Vec::new(),
                    rejected: false,
                    conflicts: Vec::new(),
                    violations: Vec::new(),
                    state: serde_json::to_value(claim_view(claim))?,
                })
            }
        }
    }

    pub(crate) fn apply_artifact_mutation(
        &self,
        prism: &Prism,
        args: PrismArtifactArgs,
        meta: EventMeta,
    ) -> Result<ArtifactMutationResult> {
        let workspace_root = self.workspace.as_ref().map(|workspace| workspace.root());
        match args.action {
            ArtifactActionInput::Propose => {
                let payload: ArtifactProposePayload = serde_json::from_value(args.payload)?;
                let task_id = CoordinationTaskId::new(payload.task_id.clone());
                let anchors = match payload.anchors {
                    Some(anchors) => convert_anchors(prism, workspace_root, anchors)?,
                    None => prism
                        .coordination_task(&task_id)
                        .map(|task| task.anchors)
                        .unwrap_or_default(),
                };
                let evidence = payload
                    .evidence
                    .unwrap_or_default()
                    .into_iter()
                    .map(EventId::new)
                    .collect::<Vec<_>>();
                let mut inferred_validated_checks = payload.validated_checks.unwrap_or_default();
                for event_id in &evidence {
                    if let Some(event) = prism.outcome_event(event_id) {
                        if matches!(event.result, prism_memory::OutcomeResult::Success) {
                            inferred_validated_checks
                                .extend(outcome_validation_labels(&event.evidence));
                        }
                    }
                }
                inferred_validated_checks.sort();
                inferred_validated_checks.dedup();
                let recipe = prism.task_validation_recipe(&task_id);
                let risk = prism.task_risk(&task_id, meta.ts);
                let (artifact_id, artifact) = prism.propose_native_artifact(
                    meta,
                    prism_coordination::ArtifactProposeInput {
                        task_id,
                        anchors,
                        diff_ref: payload.diff_ref,
                        evidence: evidence.clone(),
                        base_revision: prism.workspace_revision(),
                        current_revision: prism.workspace_revision(),
                        required_validations: payload.required_validations.unwrap_or_else(|| {
                            recipe.map(|recipe| recipe.checks).unwrap_or_default()
                        }),
                        validated_checks: inferred_validated_checks,
                        risk_score: payload
                            .risk_score
                            .or_else(|| risk.map(|risk| risk.risk_score)),
                        worktree_id: None,
                        branch_ref: None,
                    },
                )?;
                Ok(ArtifactMutationResult {
                    artifact_id: Some(artifact_id.0.to_string()),
                    review_id: None,
                    event_ids: Vec::new(),
                    rejected: false,
                    violations: Vec::new(),
                    state: serde_json::to_value(artifact_view(artifact))?,
                })
            }
            ArtifactActionInput::Supersede => {
                let payload: ArtifactSupersedePayload = serde_json::from_value(args.payload)?;
                let artifact = prism.supersede_native_artifact(
                    meta,
                    prism_coordination::ArtifactSupersedeInput {
                        artifact_id: ArtifactId::new(payload.artifact_id.clone()),
                    },
                )?;
                Ok(ArtifactMutationResult {
                    artifact_id: Some(payload.artifact_id),
                    review_id: None,
                    event_ids: Vec::new(),
                    rejected: false,
                    violations: Vec::new(),
                    state: serde_json::to_value(artifact_view(artifact))?,
                })
            }
            ArtifactActionInput::Review => {
                let payload: ArtifactReviewPayload = serde_json::from_value(args.payload)?;
                let artifact_id = ArtifactId::new(payload.artifact_id.clone());
                let risk = prism.artifact_risk(&artifact_id, meta.ts);
                let mut validated_checks = risk
                    .as_ref()
                    .map(|risk| risk.validated_checks.clone())
                    .unwrap_or_default();
                validated_checks.extend(payload.validated_checks.unwrap_or_default());
                validated_checks.sort();
                validated_checks.dedup();
                let (review_id, _, artifact) = prism.review_native_artifact(
                    meta,
                    prism_coordination::ArtifactReviewInput {
                        artifact_id,
                        verdict: convert_review_verdict(payload.verdict),
                        summary: payload.summary,
                        required_validations: payload.required_validations.unwrap_or_else(|| {
                            risk.as_ref()
                                .map(|risk| risk.required_validations.clone())
                                .unwrap_or_default()
                        }),
                        validated_checks,
                        risk_score: payload
                            .risk_score
                            .or_else(|| risk.as_ref().map(|risk| risk.risk_score)),
                    },
                    prism.workspace_revision(),
                )?;
                Ok(ArtifactMutationResult {
                    artifact_id: Some(payload.artifact_id),
                    review_id: Some(review_id.0.to_string()),
                    event_ids: Vec::new(),
                    rejected: false,
                    violations: Vec::new(),
                    state: serde_json::to_value(artifact_view(artifact))?,
                })
            }
        }
    }

    pub(crate) fn promote_curator_edge(
        &self,
        session: &SessionState,
        args: PrismCuratorPromoteEdgeArgs,
    ) -> Result<CuratorProposalDecisionResult> {
        let workspace = self
            .workspace
            .as_ref()
            .ok_or_else(|| anyhow!("curator mutations require a workspace-backed session"))?;
        let job_id = CuratorJobId(args.job_id.clone());
        let snapshot = workspace.curator_snapshot()?;
        let record = snapshot
            .records
            .iter()
            .find(|record| record.id == job_id)
            .ok_or_else(|| anyhow!("unknown curator job `{}`", args.job_id))?;
        let proposal_state = curator_proposal_state(record, args.proposal_index)?;
        if proposal_state.disposition != CuratorProposalDisposition::Pending {
            return Err(anyhow!(
                "curator proposal {} for job `{}` is already {}",
                args.proposal_index,
                args.job_id,
                curator_disposition_label(proposal_state.disposition)
            ));
        }
        let proposal = curator_proposal(record, args.proposal_index)?;
        let CuratorProposal::InferredEdge(candidate) = proposal else {
            return Err(anyhow!(
                "curator proposal {} for job `{}` is not an inferred edge",
                args.proposal_index,
                args.job_id
            ));
        };

        let task = session.task_for_mutation(args.task_id.map(TaskId::new));
        let scope =
            args.scope
                .map(convert_inferred_scope)
                .unwrap_or_else(|| match candidate.scope {
                    prism_agent::InferredEdgeScope::SessionOnly => {
                        prism_agent::InferredEdgeScope::Persisted
                    }
                    scope => scope,
                });
        let edge_id = session.inferred_edges.store_edge(
            candidate.edge.clone(),
            scope,
            Some(task.clone()),
            candidate.evidence.clone(),
        );
        if scope != prism_agent::InferredEdgeScope::SessionOnly {
            let record = session.inferred_edges.record(&edge_id).ok_or_else(|| {
                anyhow!("stored inferred edge `{}` could not be reloaded", edge_id.0)
            })?;
            workspace.append_inference_records(&[record])?;
            self.sync_inference_revision(workspace)?;
        }
        let detail = args.note.clone();
        workspace.set_curator_proposal_state(
            &job_id,
            args.proposal_index,
            CuratorProposalDisposition::Applied,
            Some(task),
            detail.clone(),
            Some(edge_id.0.clone()),
        )?;
        let proposal = self
            .curator_job(&args.job_id)?
            .and_then(|job| {
                job.proposals
                    .into_iter()
                    .find(|proposal| proposal.index == args.proposal_index)
            })
            .ok_or_else(|| anyhow!("applied curator proposal could not be reloaded"))?;
        Ok(CuratorProposalDecisionResult {
            job_id: args.job_id,
            proposal_index: args.proposal_index,
            kind: proposal.kind.clone(),
            decision: CuratorProposalDecision::Applied,
            proposal: serde_json::to_value(proposal)?,
            created: CuratorProposalCreatedResources {
                memory_id: None,
                edge_id: Some(edge_id.0.clone()),
                concept_handle: None,
            },
            detail,
            memory_id: None,
            edge_id: Some(edge_id.0),
            concept_handle: None,
        })
    }

    pub(crate) fn promote_curator_concept(
        &self,
        session: &SessionState,
        args: PrismCuratorPromoteConceptArgs,
    ) -> Result<CuratorProposalDecisionResult> {
        let workspace = self
            .workspace
            .as_ref()
            .ok_or_else(|| anyhow!("curator mutations require a workspace-backed session"))?;
        let job_id = CuratorJobId(args.job_id.clone());
        let snapshot = workspace.curator_snapshot()?;
        let record = snapshot
            .records
            .iter()
            .find(|record| record.id == job_id)
            .ok_or_else(|| anyhow!("unknown curator job `{}`", args.job_id))?;
        let proposal_state = curator_proposal_state(record, args.proposal_index)?;
        if proposal_state.disposition != CuratorProposalDisposition::Pending {
            return Err(anyhow!(
                "curator proposal {} for job `{}` is already {}",
                args.proposal_index,
                args.job_id,
                curator_disposition_label(proposal_state.disposition)
            ));
        }
        let proposal = curator_proposal(record, args.proposal_index)?;
        let CuratorProposal::ConceptCandidate(candidate) = proposal else {
            return Err(anyhow!(
                "curator proposal {} for job `{}` is not a concept candidate",
                args.proposal_index,
                args.job_id
            ));
        };

        let task_id = session.task_for_mutation(args.task_id.clone().map(TaskId::new));
        let prism = self.current_prism();
        let recorded_at = current_timestamp();
        let mut packet = build_promoted_concept_packet(
            prism.as_ref(),
            &task_id,
            recorded_at,
            concept_args_from_curator_candidate(candidate, &task_id, args.scope.clone()),
        )?;
        packet.provenance = ConceptProvenance {
            origin: "curator".to_string(),
            kind: "curator_concept_candidate".to_string(),
            task_id: Some(task_id.0.to_string()),
        };
        let event = ConceptEvent {
            id: next_concept_event_id(),
            recorded_at,
            task_id: Some(task_id.0.to_string()),
            action: ConceptEventAction::Promote,
            patch: None,
            concept: packet.clone(),
        };
        workspace.append_concept_event(event)?;
        self.sync_workspace_revision(workspace)?;
        let detail = args.note.clone();
        workspace.set_curator_proposal_state(
            &job_id,
            args.proposal_index,
            CuratorProposalDisposition::Applied,
            Some(task_id),
            detail.clone(),
            Some(packet.handle.clone()),
        )?;
        let proposal = self
            .curator_job(&args.job_id)?
            .and_then(|job| {
                job.proposals
                    .into_iter()
                    .find(|proposal| proposal.index == args.proposal_index)
            })
            .ok_or_else(|| anyhow!("applied curator proposal could not be reloaded"))?;
        Ok(CuratorProposalDecisionResult {
            job_id: args.job_id,
            proposal_index: args.proposal_index,
            kind: proposal.kind.clone(),
            decision: CuratorProposalDecision::Applied,
            proposal: serde_json::to_value(proposal)?,
            created: CuratorProposalCreatedResources {
                memory_id: None,
                edge_id: None,
                concept_handle: Some(packet.handle.clone()),
            },
            detail,
            memory_id: None,
            edge_id: None,
            concept_handle: Some(packet.handle),
        })
    }

    pub(crate) fn apply_curator_proposal(
        &self,
        session: &SessionState,
        args: PrismCuratorApplyProposalArgs,
    ) -> Result<CuratorProposalDecisionResult> {
        let workspace = self
            .workspace
            .as_ref()
            .ok_or_else(|| anyhow!("curator mutations require a workspace-backed session"))?;
        let job_id = CuratorJobId(args.job_id.clone());
        let snapshot = workspace.curator_snapshot()?;
        let record = snapshot
            .records
            .iter()
            .find(|record| record.id == job_id)
            .ok_or_else(|| anyhow!("unknown curator job `{}`", args.job_id))?;
        let proposal_state = curator_proposal_state(record, args.proposal_index)?;
        if proposal_state.disposition != CuratorProposalDisposition::Pending {
            return Err(anyhow!(
                "curator proposal {} for job `{}` is already {}",
                args.proposal_index,
                args.job_id,
                curator_disposition_label(proposal_state.disposition)
            ));
        }

        let proposal = curator_proposal(record, args.proposal_index)?;
        let options = args.options;

        match proposal {
            CuratorProposal::InferredEdge(_) => self.promote_curator_edge(
                session,
                PrismCuratorPromoteEdgeArgs {
                    job_id: args.job_id,
                    proposal_index: args.proposal_index,
                    scope: options
                        .as_ref()
                        .and_then(|options| options.edge_scope.clone()),
                    note: args.note,
                    task_id: args.task_id,
                },
            ),
            CuratorProposal::ConceptCandidate(_) => self.promote_curator_concept(
                session,
                PrismCuratorPromoteConceptArgs {
                    job_id: args.job_id,
                    proposal_index: args.proposal_index,
                    scope: options
                        .as_ref()
                        .and_then(|options| options.concept_scope.clone()),
                    note: args.note,
                    task_id: args.task_id,
                },
            ),
            CuratorProposal::StructuralMemory(_)
            | CuratorProposal::SemanticMemory(_)
            | CuratorProposal::RiskSummary(_)
            | CuratorProposal::ValidationRecipe(_) => self.promote_curator_memory(
                session,
                PrismCuratorPromoteMemoryArgs {
                    job_id: args.job_id,
                    proposal_index: args.proposal_index,
                    trust: options.as_ref().and_then(|options| options.memory_trust),
                    note: args.note,
                    task_id: args.task_id,
                },
            ),
        }
    }

    pub(crate) fn promote_curator_memory(
        &self,
        session: &SessionState,
        args: PrismCuratorPromoteMemoryArgs,
    ) -> Result<CuratorProposalDecisionResult> {
        let workspace = self
            .workspace
            .as_ref()
            .ok_or_else(|| anyhow!("curator mutations require a workspace-backed session"))?;
        let job_id = CuratorJobId(args.job_id.clone());
        let snapshot = workspace.curator_snapshot()?;
        let record = snapshot
            .records
            .iter()
            .find(|record| record.id == job_id)
            .ok_or_else(|| anyhow!("unknown curator job `{}`", args.job_id))?;
        let proposal_state = curator_proposal_state(record, args.proposal_index)?;
        if proposal_state.disposition != CuratorProposalDisposition::Pending {
            return Err(anyhow!(
                "curator proposal {} for job `{}` is already {}",
                args.proposal_index,
                args.job_id,
                curator_disposition_label(proposal_state.disposition)
            ));
        }

        let task = session.task_for_mutation(args.task_id.clone().map(TaskId::new));
        let prism = self.current_prism();
        let proposal = curator_proposal(record, args.proposal_index)?;
        let (entry, promoted_from) = match proposal {
            CuratorProposal::StructuralMemory(candidate) => {
                let mut entry = MemoryEntry::new(candidate.kind, candidate.content.clone());
                entry.anchors = prism.anchors_for(&candidate.anchors);
                entry.scope = MemoryScope::Repo;
                entry.source = MemorySource::System;
                entry.trust = args.trust.unwrap_or(candidate.trust).clamp(0.0, 1.0);
                entry.metadata = curator_memory_metadata(
                    proposal,
                    candidate,
                    &task,
                    &args.job_id,
                    args.proposal_index,
                    Value::Null,
                );
                entry.metadata =
                    ensure_repo_publication_metadata(entry.metadata, current_timestamp());
                (entry, candidate.evidence.memory_ids.clone())
            }
            CuratorProposal::SemanticMemory(candidate) => {
                let mut entry = MemoryEntry::new(candidate.kind, candidate.content.clone());
                entry.anchors = prism.anchors_for(&candidate.anchors);
                entry.scope = MemoryScope::Repo;
                entry.source = MemorySource::System;
                entry.trust = args.trust.unwrap_or(candidate.trust).clamp(0.0, 1.0);
                entry.metadata = curator_memory_metadata(
                    proposal,
                    candidate,
                    &task,
                    &args.job_id,
                    args.proposal_index,
                    Value::Null,
                );
                entry.metadata =
                    ensure_repo_publication_metadata(entry.metadata, current_timestamp());
                (entry, candidate.evidence.memory_ids.clone())
            }
            CuratorProposal::RiskSummary(candidate) => {
                let candidate_memory = prism_curator::CandidateMemory {
                    anchors: candidate.anchors.clone(),
                    kind: MemoryKind::Semantic,
                    content: candidate.summary.clone(),
                    trust: match candidate.severity.as_str() {
                        "low" => 0.55,
                        "medium" => 0.7,
                        "high" => 0.85,
                        _ => 0.6,
                    },
                    rationale: "Curator promoted a semantic risk summary.".to_string(),
                    category: Some("risk_summary".to_string()),
                    evidence: prism_curator::CandidateMemoryEvidence {
                        event_ids: candidate.evidence_events.clone(),
                        memory_ids: Vec::new(),
                        validation_checks: Vec::new(),
                        co_change_lineages: Vec::new(),
                    },
                };
                let mut entry =
                    MemoryEntry::new(MemoryKind::Semantic, candidate_memory.content.clone());
                entry.anchors = prism.anchors_for(&candidate.anchors);
                entry.scope = MemoryScope::Repo;
                entry.source = MemorySource::System;
                entry.trust = args.trust.unwrap_or(candidate_memory.trust).clamp(0.0, 1.0);
                entry.metadata = curator_memory_metadata(
                    proposal,
                    &candidate_memory,
                    &task,
                    &args.job_id,
                    args.proposal_index,
                    json!({
                        "severity": candidate.severity,
                        "evidenceEvents": candidate
                            .evidence_events
                            .iter()
                            .map(|event| event.0.clone())
                            .collect::<Vec<_>>(),
                    }),
                );
                entry.metadata =
                    ensure_repo_publication_metadata(entry.metadata, current_timestamp());
                (entry, candidate_memory.evidence.memory_ids.clone())
            }
            CuratorProposal::ValidationRecipe(candidate) => {
                let candidate_memory = prism_curator::CandidateMemory {
                    anchors: vec![AnchorRef::Node(candidate.target.clone())],
                    kind: MemoryKind::Structural,
                    content: format!(
                        "Validation recipe for {}: {}",
                        candidate.target.path,
                        candidate.checks.join(", ")
                    ),
                    trust: 0.8,
                    rationale: candidate.rationale.clone(),
                    category: Some("validation_recipe".to_string()),
                    evidence: prism_curator::CandidateMemoryEvidence {
                        event_ids: Vec::new(),
                        memory_ids: Vec::new(),
                        validation_checks: candidate.checks.clone(),
                        co_change_lineages: Vec::new(),
                    },
                };
                let mut entry =
                    MemoryEntry::new(MemoryKind::Structural, candidate_memory.content.clone());
                entry.anchors = prism.anchors_for(&[AnchorRef::Node(candidate.target.clone())]);
                entry.scope = MemoryScope::Repo;
                entry.source = MemorySource::System;
                entry.trust = args.trust.unwrap_or(0.8).clamp(0.0, 1.0);
                entry.metadata = curator_memory_metadata(
                    proposal,
                    &candidate_memory,
                    &task,
                    &args.job_id,
                    args.proposal_index,
                    json!({
                        "target": candidate.target,
                        "checks": candidate.checks,
                        "evidence": candidate.evidence,
                    }),
                );
                entry.metadata =
                    ensure_repo_publication_metadata(entry.metadata, current_timestamp());
                (entry, candidate_memory.evidence.memory_ids.clone())
            }
            CuratorProposal::InferredEdge(_) => {
                return Err(anyhow!(
                    "curator proposal {} for job `{}` is an inferred edge; use prism_mutate with action `curator_promote_edge`",
                    args.proposal_index,
                    args.job_id
                ));
            }
            CuratorProposal::ConceptCandidate(_) => {
                return Err(anyhow!(
                    "curator proposal {} for job `{}` is a concept candidate; use prism_mutate with action `curator_promote_concept`",
                    args.proposal_index,
                    args.job_id
                ));
            }
        };
        let memory_summary = entry.content.clone();
        let memory_anchors = entry.anchors.clone();
        ensure_repo_memory_publication_is_not_duplicate(session, &entry, &[])?;
        let memory_id = session.notes.store(entry)?;
        let stored_entry = session
            .notes
            .entry(&memory_id)
            .ok_or_else(|| anyhow!("promoted memory `{}` could not be reloaded", memory_id.0))?;
        workspace.append_memory_event(MemoryEvent::from_entry(
            MemoryEventKind::Promoted,
            stored_entry.clone(),
            Some(task.0.to_string()),
            promoted_from,
            Vec::new(),
        ))?;
        self.reload_episodic_snapshot(workspace)?;
        let note_event = OutcomeEvent {
            meta: EventMeta {
                id: session.next_event_id("outcome"),
                ts: current_timestamp(),
                actor: EventActor::System,
                correlation: Some(task.clone()),
                causation: None,
            },
            anchors: memory_anchors,
            kind: prism_memory::OutcomeKind::NoteAdded,
            result: prism_memory::OutcomeResult::Success,
            summary: memory_summary,
            evidence: Vec::new(),
            metadata: json!({
                "source": "curator",
                "memoryId": memory_id.0.clone(),
                "jobId": args.job_id,
                "proposalIndex": args.proposal_index,
            }),
        };
        if let Some(workspace) = &self.workspace {
            let _ = workspace.append_outcome(note_event)?;
            self.sync_workspace_revision(workspace)?;
        } else {
            prism.apply_outcome_event_to_projections(&note_event);
            let _ = prism.outcome_memory().store_event(note_event)?;
            self.persist_outcomes()?;
            self.persist_notes()?;
        }
        let detail = args.note.clone();
        workspace.set_curator_proposal_state(
            &job_id,
            args.proposal_index,
            CuratorProposalDisposition::Applied,
            Some(task),
            detail.clone(),
            Some(memory_id.0.clone()),
        )?;
        let proposal = self
            .curator_job(&args.job_id)?
            .and_then(|job| {
                job.proposals
                    .into_iter()
                    .find(|proposal| proposal.index == args.proposal_index)
            })
            .ok_or_else(|| anyhow!("applied curator proposal could not be reloaded"))?;
        Ok(CuratorProposalDecisionResult {
            job_id: args.job_id,
            proposal_index: args.proposal_index,
            kind: proposal.kind.clone(),
            decision: CuratorProposalDecision::Applied,
            proposal: serde_json::to_value(proposal)?,
            created: CuratorProposalCreatedResources {
                memory_id: Some(memory_id.0.clone()),
                edge_id: None,
                concept_handle: None,
            },
            detail,
            memory_id: Some(memory_id.0),
            edge_id: None,
            concept_handle: None,
        })
    }

    pub(crate) fn reject_curator_proposal(
        &self,
        session: &SessionState,
        args: PrismCuratorRejectProposalArgs,
    ) -> Result<CuratorProposalDecisionResult> {
        let workspace = self
            .workspace
            .as_ref()
            .ok_or_else(|| anyhow!("curator mutations require a workspace-backed session"))?;
        let job_id = CuratorJobId(args.job_id.clone());
        let snapshot = workspace.curator_snapshot()?;
        let record = snapshot
            .records
            .iter()
            .find(|record| record.id == job_id)
            .ok_or_else(|| anyhow!("unknown curator job `{}`", args.job_id))?;
        let proposal_state = curator_proposal_state(record, args.proposal_index)?;
        if proposal_state.disposition != CuratorProposalDisposition::Pending {
            return Err(anyhow!(
                "curator proposal {} for job `{}` is already {}",
                args.proposal_index,
                args.job_id,
                curator_disposition_label(proposal_state.disposition)
            ));
        }

        let task = session.task_for_mutation(args.task_id.map(TaskId::new));
        let detail = args.reason.clone();
        workspace.set_curator_proposal_state(
            &job_id,
            args.proposal_index,
            CuratorProposalDisposition::Rejected,
            Some(task),
            detail.clone(),
            None,
        )?;
        let proposal = self
            .curator_job(&args.job_id)?
            .and_then(|job| {
                job.proposals
                    .into_iter()
                    .find(|proposal| proposal.index == args.proposal_index)
            })
            .ok_or_else(|| anyhow!("rejected curator proposal could not be reloaded"))?;
        Ok(CuratorProposalDecisionResult {
            job_id: args.job_id,
            proposal_index: args.proposal_index,
            kind: proposal.kind.clone(),
            decision: CuratorProposalDecision::Rejected,
            proposal: serde_json::to_value(proposal)?,
            created: CuratorProposalCreatedResources::default(),
            detail,
            memory_id: None,
            edge_id: None,
            concept_handle: None,
        })
    }

    pub(crate) fn curator_jobs(&self, args: crate::CuratorJobsArgs) -> Result<Vec<CuratorJobView>> {
        self.refresh_workspace()?;
        let Some(workspace) = &self.workspace else {
            return Ok(Vec::new());
        };
        let mut jobs = workspace
            .curator_snapshot()?
            .records
            .into_iter()
            .filter(|record| {
                args.status
                    .as_deref()
                    .is_none_or(|status| curator_job_status_label(record) == status)
                    && args
                        .trigger
                        .as_deref()
                        .is_none_or(|trigger| curator_trigger_label(&record.job.trigger) == trigger)
            })
            .map(crate::curator_job_view)
            .collect::<Result<Vec<_>>>()?;

        jobs.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        if let Some(limit) = args.limit {
            jobs.truncate(limit);
        }
        Ok(jobs)
    }

    pub(crate) fn curator_proposals(
        &self,
        args: crate::CuratorProposalsArgs,
    ) -> Result<Vec<CuratorProposalRecordView>> {
        self.refresh_workspace()?;
        let Some(workspace) = &self.workspace else {
            return Ok(Vec::new());
        };
        let mut proposals = Vec::new();
        for record in workspace.curator_snapshot()?.records {
            if args
                .status
                .as_deref()
                .is_some_and(|status| curator_job_status_label(&record) != status)
                || args
                    .trigger
                    .as_deref()
                    .is_some_and(|trigger| curator_trigger_label(&record.job.trigger) != trigger)
            {
                continue;
            }
            let run = record.run.clone().unwrap_or_default();
            for (index, proposal) in run.proposals.into_iter().enumerate() {
                let state = record
                    .proposal_states
                    .get(index)
                    .cloned()
                    .unwrap_or_default();
                if args.disposition.as_deref().is_some_and(|disposition| {
                    curator_disposition_label(state.disposition) != disposition
                }) || args.task_id.as_deref().is_some_and(|task_id| {
                    record.job.task.as_ref().map(|task| task.0.as_str()) != Some(task_id)
                        && state.task.as_ref().map(|task| task.0.as_str()) != Some(task_id)
                }) {
                    continue;
                }
                let proposal_view =
                    crate::curator_proposal_record_view(&record, index, proposal, state)?;
                if args
                    .kind
                    .as_deref()
                    .is_none_or(|kind| proposal_view.kind == kind)
                {
                    proposals.push(proposal_view);
                }
            }
        }

        proposals.sort_by(|left, right| {
            right
                .job_created_at
                .cmp(&left.job_created_at)
                .then_with(|| left.index.cmp(&right.index))
        });
        if let Some(limit) = args.limit {
            proposals.truncate(limit);
        }
        Ok(proposals)
    }

    pub(crate) fn curator_job(&self, job_id: &str) -> Result<Option<CuratorJobView>> {
        self.refresh_workspace()?;
        let Some(workspace) = &self.workspace else {
            return Ok(None);
        };
        workspace
            .curator_snapshot()?
            .records
            .into_iter()
            .find(|record| record.id.0 == job_id)
            .map(crate::curator_job_view)
            .transpose()
    }
}

fn memory_event_kind_for_store(
    promoted_from: &[prism_memory::MemoryId],
    supersedes: &[prism_memory::MemoryId],
) -> MemoryEventKind {
    if !supersedes.is_empty() && promoted_from.is_empty() {
        MemoryEventKind::Superseded
    } else if !promoted_from.is_empty() || !supersedes.is_empty() {
        MemoryEventKind::Promoted
    } else {
        MemoryEventKind::Stored
    }
}

fn ensure_repo_memory_publication_is_not_duplicate(
    session: &SessionState,
    entry: &MemoryEntry,
    supersedes: &[prism_memory::MemoryId],
) -> Result<()> {
    if entry.scope != MemoryScope::Repo {
        return Ok(());
    }
    let duplicate_ids = session
        .notes
        .snapshot()
        .entries
        .into_iter()
        .filter(|existing| existing.scope == MemoryScope::Repo)
        .filter(|existing| existing.kind == entry.kind)
        .filter(|existing| !supersedes.iter().any(|id| id == &existing.id))
        .filter(|existing| memory_publication_status(existing) != Some("retired"))
        .filter(|existing| entries_share_anchor(existing, entry))
        .filter(|existing| {
            normalize_memory_content(&existing.content) == normalize_memory_content(&entry.content)
        })
        .map(|existing| existing.id.0)
        .collect::<Vec<_>>();
    if duplicate_ids.is_empty() {
        return Ok(());
    }
    Err(anyhow!(
        "repo-published memory duplicates active published memory {}. Add `supersedes` to publish a reviewed replacement, or retire the older memory first.",
        duplicate_ids.join(", ")
    ))
}

fn entries_share_anchor(left: &MemoryEntry, right: &MemoryEntry) -> bool {
    left.anchors
        .iter()
        .any(|anchor| right.anchors.iter().any(|candidate| candidate == anchor))
}

fn memory_publication_status(entry: &MemoryEntry) -> Option<&str> {
    entry
        .metadata
        .get("publication")
        .and_then(|value| value.get("status"))
        .and_then(Value::as_str)
}

fn normalize_memory_content(content: &str) -> String {
    content
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn outcome_validation_labels(evidence: &[OutcomeEvidence]) -> Vec<String> {
    let mut labels = evidence
        .iter()
        .filter_map(|evidence| match evidence {
            OutcomeEvidence::Test { name, .. } => Some(normalize_validation_label(name, "test:")),
            OutcomeEvidence::Build { target, .. } => {
                Some(normalize_validation_label(target, "build:"))
            }
            OutcomeEvidence::Command { argv, passed } if *passed => match argv.as_slice() {
                [tool, subcommand, ..] if tool == "cargo" && subcommand == "test" => {
                    Some(format!("test:{}", argv.join(" ")))
                }
                [tool, subcommand, ..] if tool == "cargo" && subcommand == "build" => {
                    Some(format!("build:{}", argv.join(" ")))
                }
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();
    labels.sort();
    labels.dedup();
    labels
}

fn normalize_validation_label(value: &str, default_prefix: &str) -> String {
    let value = value.trim();
    if value.starts_with("test:")
        || value.starts_with("build:")
        || value.starts_with("validation:")
        || value.starts_with("command:")
    {
        value.to_string()
    } else {
        format!("{default_prefix}{value}")
    }
}

fn build_promoted_concept_packet(
    prism: &Prism,
    task_id: &TaskId,
    recorded_at: u64,
    args: PrismConceptMutationArgs,
) -> Result<ConceptPacket> {
    let scope = args
        .scope
        .clone()
        .map(convert_concept_scope)
        .unwrap_or(ConceptScope::Session);
    let canonical_name = args
        .canonical_name
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("concept promote requires canonicalName"))?;
    let summary = args
        .summary
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("concept promote requires summary"))?;
    let core_members = args
        .core_members
        .ok_or_else(|| anyhow!("concept promote requires coreMembers"))?;

    let core_members = convert_concept_nodes(prism, core_members, "coreMembers")?;
    let supporting_members =
        convert_optional_concept_nodes(prism, args.supporting_members, "supportingMembers")?;
    let likely_tests = convert_optional_concept_nodes(prism, args.likely_tests, "likelyTests")?;
    let risk_hint = match parse_sparse_patch(args.risk_hint, "riskHint")? {
        SparsePatch::Keep | SparsePatch::Clear => None,
        SparsePatch::Set(value) => {
            let risk_hint = value.trim().to_string();
            if risk_hint.is_empty() {
                return Err(anyhow!("concept riskHint cannot be empty"));
            }
            Some(risk_hint)
        }
    };

    let packet = ConceptPacket {
        handle: normalize_concept_handle(args.handle.as_deref(), &canonical_name),
        canonical_name,
        summary,
        aliases: sanitize_strings(args.aliases.unwrap_or_default()),
        confidence: args.confidence.unwrap_or(0.88).clamp(0.0, 1.0),
        core_members: core_members.clone(),
        core_member_lineages: concept_member_lineages(prism, &core_members),
        supporting_members: supporting_members.clone(),
        supporting_member_lineages: concept_member_lineages(prism, &supporting_members),
        likely_tests: likely_tests.clone(),
        likely_test_lineages: concept_member_lineages(prism, &likely_tests),
        evidence: sanitize_strings(args.evidence.unwrap_or_else(|| {
            vec!["Promoted from live repo work through prism_mutate.".to_string()]
        })),
        risk_hint,
        decode_lenses: convert_concept_lenses(args.decode_lenses),
        scope,
        provenance: ConceptProvenance {
            origin: match scope {
                ConceptScope::Local => "local_mutation".to_string(),
                ConceptScope::Session => "session_mutation".to_string(),
                ConceptScope::Repo => "repo_mutation".to_string(),
            },
            kind: "manual_concept_promote".to_string(),
            task_id: Some(task_id.0.to_string()),
        },
        publication: (scope == ConceptScope::Repo).then_some(ConceptPublication {
            published_at: recorded_at,
            last_reviewed_at: Some(recorded_at),
            status: ConceptPublicationStatus::Active,
            supersedes: normalize_concept_handles(args.supersedes.unwrap_or_default()),
            retired_at: None,
            retirement_reason: None,
        }),
    };
    validate_concept_packet(&packet)?;
    Ok(packet)
}

fn build_promoted_contract_packet(
    prism: &Prism,
    workspace_root: Option<&std::path::Path>,
    task_id: &TaskId,
    recorded_at: u64,
    args: PrismContractMutationArgs,
) -> Result<ContractPacket> {
    let scope = args
        .scope
        .clone()
        .map(convert_concept_scope)
        .unwrap_or(ConceptScope::Session);
    let name = args
        .name
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("contract promote requires name"))?;
    let summary = args
        .summary
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("contract promote requires summary"))?;
    let kind = args
        .kind
        .clone()
        .map(convert_contract_kind)
        .ok_or_else(|| anyhow!("contract promote requires kind"))?;
    let subject = convert_contract_target(
        prism,
        workspace_root,
        args.subject
            .clone()
            .ok_or_else(|| anyhow!("contract promote requires subject"))?,
    )?;
    let guarantees = convert_contract_guarantees(
        args.guarantees
            .clone()
            .ok_or_else(|| anyhow!("contract promote requires guarantees"))?,
    )?;
    let status = args
        .status
        .clone()
        .map(convert_contract_status)
        .unwrap_or_else(|| {
            if scope == ConceptScope::Repo {
                ContractStatus::Active
            } else {
                ContractStatus::Candidate
            }
        });

    let packet = ContractPacket {
        handle: normalize_contract_handle(args.handle.as_deref(), &name),
        name,
        summary,
        aliases: sanitize_strings(args.aliases.unwrap_or_default()),
        kind,
        subject,
        guarantees,
        assumptions: sanitize_strings(args.assumptions.unwrap_or_default()),
        consumers: convert_contract_targets(prism, workspace_root, args.consumers)?,
        validations: convert_contract_validations(prism, workspace_root, args.validations)?,
        stability: args
            .stability
            .clone()
            .map(convert_contract_stability)
            .unwrap_or(ContractStability::Internal),
        compatibility: args
            .compatibility
            .map(convert_contract_compatibility)
            .unwrap_or_default(),
        evidence: sanitize_strings(args.evidence.unwrap_or_else(|| {
            vec!["Promoted from live repo work through prism_mutate.".to_string()]
        })),
        status,
        scope,
        provenance: ConceptProvenance {
            origin: match scope {
                ConceptScope::Local => "local_mutation".to_string(),
                ConceptScope::Session => "session_mutation".to_string(),
                ConceptScope::Repo => "repo_mutation".to_string(),
            },
            kind: "manual_contract_promote".to_string(),
            task_id: Some(task_id.0.to_string()),
        },
        publication: (scope == ConceptScope::Repo).then_some(ConceptPublication {
            published_at: recorded_at,
            last_reviewed_at: Some(recorded_at),
            status: if status == ContractStatus::Retired {
                ConceptPublicationStatus::Retired
            } else {
                ConceptPublicationStatus::Active
            },
            supersedes: normalize_contract_handles(args.supersedes.unwrap_or_default()),
            retired_at: (status == ContractStatus::Retired).then_some(recorded_at),
            retirement_reason: args.retirement_reason.clone(),
        }),
    };
    validate_contract_packet(&packet)?;
    Ok(packet)
}

fn build_updated_contract_packet(
    prism: &Prism,
    workspace_root: Option<&std::path::Path>,
    task_id: &TaskId,
    recorded_at: u64,
    args: PrismContractMutationArgs,
) -> Result<ContractPacket> {
    let handle =
        required_contract_handle(args.handle.as_deref(), "contract update requires handle")?;
    let mut packet = current_contract(prism, &handle)?;
    let mut changed = false;

    if let Some(name) = args
        .name
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        packet.name = name;
        changed = true;
    }
    if let Some(summary) = args
        .summary
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        packet.summary = summary;
        changed = true;
    }
    if let Some(aliases) = args.aliases {
        packet.aliases = sanitize_strings(aliases);
        changed = true;
    }
    if let Some(kind) = args.kind {
        packet.kind = convert_contract_kind(kind);
        changed = true;
    }
    if let Some(subject) = args.subject {
        packet.subject = convert_contract_target(prism, workspace_root, subject)?;
        changed = true;
    }
    if let Some(guarantees) = args.guarantees {
        packet.guarantees = convert_contract_guarantees(guarantees)?;
        changed = true;
    }
    if let Some(assumptions) = args.assumptions {
        packet.assumptions = sanitize_strings(assumptions);
        changed = true;
    }
    if let Some(consumers) = args.consumers {
        packet.consumers = convert_contract_targets(prism, workspace_root, Some(consumers))?;
        changed = true;
    }
    if let Some(validations) = args.validations {
        packet.validations =
            convert_contract_validations(prism, workspace_root, Some(validations))?;
        changed = true;
    }
    if let Some(stability) = args.stability {
        packet.stability = convert_contract_stability(stability);
        changed = true;
    }
    if let Some(compatibility) = args.compatibility {
        packet.compatibility = convert_contract_compatibility(compatibility);
        changed = true;
    }
    if let Some(evidence) = args.evidence {
        packet.evidence = sanitize_strings(evidence);
        changed = true;
    }
    if let Some(status) = args.status {
        packet.status = convert_contract_status(status);
        changed = true;
    }
    if let Some(scope) = args.scope.map(convert_concept_scope) {
        packet.scope = scope;
        changed = true;
    }
    if let Some(supersedes) = args.supersedes {
        let publication = packet
            .publication
            .get_or_insert_with(ConceptPublication::default);
        publication.supersedes = normalize_contract_handles(supersedes);
        changed = true;
    }
    if !changed {
        return Err(anyhow!(
            "contract update requires at least one changed field"
        ));
    }
    packet.provenance = ConceptProvenance {
        origin: match packet.scope {
            ConceptScope::Local => "local_mutation".to_string(),
            ConceptScope::Session => "session_mutation".to_string(),
            ConceptScope::Repo => "repo_mutation".to_string(),
        },
        kind: "manual_contract_update".to_string(),
        task_id: Some(task_id.0.to_string()),
    };
    packet.publication = update_contract_publication(
        packet.publication,
        packet.scope,
        packet.status,
        recorded_at,
        None,
    );
    validate_contract_packet(&packet)?;
    Ok(packet)
}

fn build_retired_contract_packet(
    prism: &Prism,
    task_id: &TaskId,
    recorded_at: u64,
    args: PrismContractMutationArgs,
) -> Result<ContractPacket> {
    let handle =
        required_contract_handle(args.handle.as_deref(), "contract retire requires handle")?;
    let mut packet = current_contract(prism, &handle)?;
    packet.status = ContractStatus::Retired;
    packet.provenance = ConceptProvenance {
        origin: match packet.scope {
            ConceptScope::Local => "local_mutation".to_string(),
            ConceptScope::Session => "session_mutation".to_string(),
            ConceptScope::Repo => "repo_mutation".to_string(),
        },
        kind: "manual_contract_retire".to_string(),
        task_id: Some(task_id.0.to_string()),
    };
    packet.publication = update_contract_publication(
        packet.publication,
        packet.scope,
        packet.status,
        recorded_at,
        args.retirement_reason.clone(),
    );
    validate_contract_packet(&packet)?;
    Ok(packet)
}

fn build_contract_with_evidence_attached(
    prism: &Prism,
    task_id: &TaskId,
    recorded_at: u64,
    args: PrismContractMutationArgs,
) -> Result<ContractPacket> {
    let additions = sanitize_strings(
        args.evidence
            .clone()
            .ok_or_else(|| anyhow!("attach_evidence requires evidence"))?,
    );
    if additions.is_empty() {
        return Err(anyhow!("attach_evidence requires non-empty evidence"));
    }
    let handle =
        required_contract_handle(args.handle.as_deref(), "attach_evidence requires handle")?;
    let mut packet = current_contract(prism, &handle)?;
    packet.evidence = merge_unique_strings(packet.evidence, additions);
    packet.provenance = ConceptProvenance {
        origin: origin_for_scope(packet.scope).to_string(),
        kind: "manual_contract_attach_evidence".to_string(),
        task_id: Some(task_id.0.to_string()),
    };
    packet.publication = update_contract_publication(
        packet.publication,
        packet.scope,
        packet.status,
        recorded_at,
        None,
    );
    validate_contract_packet(&packet)?;
    Ok(packet)
}

fn build_contract_with_validation_attached(
    prism: &Prism,
    workspace_root: Option<&std::path::Path>,
    task_id: &TaskId,
    recorded_at: u64,
    args: PrismContractMutationArgs,
) -> Result<ContractPacket> {
    let additions = convert_contract_validations(prism, workspace_root, args.validations.clone())?;
    if additions.is_empty() {
        return Err(anyhow!("attach_validation requires validations"));
    }
    let handle =
        required_contract_handle(args.handle.as_deref(), "attach_validation requires handle")?;
    let mut packet = current_contract(prism, &handle)?;
    packet.validations = merge_contract_validations(packet.validations, additions);
    packet.provenance = ConceptProvenance {
        origin: origin_for_scope(packet.scope).to_string(),
        kind: "manual_contract_attach_validation".to_string(),
        task_id: Some(task_id.0.to_string()),
    };
    packet.publication = update_contract_publication(
        packet.publication,
        packet.scope,
        packet.status,
        recorded_at,
        None,
    );
    validate_contract_packet(&packet)?;
    Ok(packet)
}

fn build_contract_with_consumer_recorded(
    prism: &Prism,
    workspace_root: Option<&std::path::Path>,
    task_id: &TaskId,
    recorded_at: u64,
    args: PrismContractMutationArgs,
) -> Result<ContractPacket> {
    let additions = convert_contract_targets(prism, workspace_root, args.consumers.clone())?;
    if additions.is_empty() {
        return Err(anyhow!("record_consumer requires consumers"));
    }
    let handle =
        required_contract_handle(args.handle.as_deref(), "record_consumer requires handle")?;
    let mut packet = current_contract(prism, &handle)?;
    packet.consumers = merge_contract_targets(packet.consumers, additions);
    packet.provenance = ConceptProvenance {
        origin: origin_for_scope(packet.scope).to_string(),
        kind: "manual_contract_record_consumer".to_string(),
        task_id: Some(task_id.0.to_string()),
    };
    packet.publication = update_contract_publication(
        packet.publication,
        packet.scope,
        packet.status,
        recorded_at,
        None,
    );
    validate_contract_packet(&packet)?;
    Ok(packet)
}

fn build_contract_with_status_set(
    prism: &Prism,
    task_id: &TaskId,
    recorded_at: u64,
    args: PrismContractMutationArgs,
) -> Result<ContractPacket> {
    let status = args
        .status
        .clone()
        .map(convert_contract_status)
        .ok_or_else(|| anyhow!("set_status requires status"))?;
    let handle = required_contract_handle(args.handle.as_deref(), "set_status requires handle")?;
    let mut packet = current_contract(prism, &handle)?;
    packet.status = status;
    packet.provenance = ConceptProvenance {
        origin: origin_for_scope(packet.scope).to_string(),
        kind: "manual_contract_set_status".to_string(),
        task_id: Some(task_id.0.to_string()),
    };
    packet.publication = update_contract_publication(
        packet.publication,
        packet.scope,
        packet.status,
        recorded_at,
        None,
    );
    validate_contract_packet(&packet)?;
    Ok(packet)
}

fn concept_args_from_curator_candidate(
    candidate: &CandidateConcept,
    task_id: &TaskId,
    scope: Option<ConceptScopeInput>,
) -> PrismConceptMutationArgs {
    PrismConceptMutationArgs {
        operation: match candidate.recommended_operation {
            CandidateConceptOperation::Promote => ConceptMutationOperationInput::Promote,
        },
        handle: None,
        canonical_name: Some(candidate.canonical_name.clone()),
        summary: Some(candidate.summary.clone()),
        aliases: (!candidate.aliases.is_empty()).then_some(candidate.aliases.clone()),
        core_members: Some(
            candidate
                .core_members
                .iter()
                .cloned()
                .map(node_id_input)
                .collect(),
        ),
        supporting_members: (!candidate.supporting_members.is_empty()).then_some(
            candidate
                .supporting_members
                .iter()
                .cloned()
                .map(node_id_input)
                .collect(),
        ),
        likely_tests: (!candidate.likely_tests.is_empty()).then_some(
            candidate
                .likely_tests
                .iter()
                .cloned()
                .map(node_id_input)
                .collect(),
        ),
        evidence: (!candidate.evidence.is_empty()).then_some(candidate.evidence.clone()),
        risk_hint: None,
        confidence: Some(candidate.confidence),
        decode_lenses: Some(vec![
            PrismConceptLensInput::Open,
            PrismConceptLensInput::Workset,
            PrismConceptLensInput::Validation,
        ]),
        scope: Some(scope.unwrap_or(ConceptScopeInput::Session)),
        supersedes: None,
        retirement_reason: None,
        task_id: Some(task_id.0.to_string()),
    }
}

fn build_concept_relation(
    prism: &Prism,
    task_id: &TaskId,
    args: &PrismConceptRelationMutationArgs,
) -> Result<ConceptRelation> {
    let source_handle = normalize_concept_handle(Some(&args.source_handle), &args.source_handle);
    let target_handle = normalize_concept_handle(Some(&args.target_handle), &args.target_handle);
    if source_handle == target_handle {
        return Err(anyhow!(
            "concept relations require distinct source and target handles"
        ));
    }
    prism
        .concept_by_handle(&source_handle)
        .ok_or_else(|| anyhow!("no concept packet matched `{source_handle}`"))?;
    prism
        .concept_by_handle(&target_handle)
        .ok_or_else(|| anyhow!("no concept packet matched `{target_handle}`"))?;
    let kind = convert_concept_relation_kind(args.kind.clone());
    match args.operation {
        ConceptRelationMutationOperationInput::Upsert => Ok(ConceptRelation {
            source_handle,
            target_handle,
            kind,
            confidence: args.confidence.unwrap_or(0.78).clamp(0.0, 1.0),
            evidence: sanitize_strings(args.evidence.clone().unwrap_or_default()),
            scope: args
                .scope
                .clone()
                .map(convert_concept_scope)
                .unwrap_or(ConceptScope::Session),
            provenance: ConceptProvenance {
                origin: "manual_concept_relation".to_string(),
                kind: "manual_concept_relation".to_string(),
                task_id: Some(task_id.0.to_string()),
            },
        }),
        ConceptRelationMutationOperationInput::Retire => prism
            .concept_relations_for_handle(&source_handle)
            .into_iter()
            .find(|relation| {
                relation.source_handle.eq_ignore_ascii_case(&source_handle)
                    && relation.target_handle.eq_ignore_ascii_case(&target_handle)
                    && relation.kind == kind
            })
            .ok_or_else(|| {
                anyhow!(
                    "no concept relation matched `{}` -> `{}` ({:?})",
                    source_handle,
                    target_handle,
                    kind
                )
            }),
    }
}

fn node_id_input(id: prism_ir::NodeId) -> NodeIdInput {
    NodeIdInput {
        crate_name: id.crate_name.to_string(),
        path: id.path.to_string(),
        kind: id.kind.to_string(),
    }
}

fn parse_sparse_patch<T>(
    value: Option<SparsePatchInput<T>>,
    field: &str,
) -> Result<SparsePatch<T>> {
    value
        .map(|patch| patch.into_patch(field))
        .transpose()
        .map_err(|error| anyhow!(error))?
        .map_or(Ok(SparsePatch::Keep), Ok)
}

fn concept_event_patch(
    args: &PrismConceptMutationArgs,
    operation: &ConceptMutationOperationInput,
    packet: &ConceptPacket,
) -> Result<Option<ConceptEventPatch>> {
    let mut patch = ConceptEventPatch::default();
    match operation {
        ConceptMutationOperationInput::Promote => return Ok(None),
        ConceptMutationOperationInput::Update => {
            if args.canonical_name.is_some() {
                patch.set_fields.push("canonicalName".to_string());
                patch.canonical_name = Some(packet.canonical_name.clone());
            }
            if args.summary.is_some() {
                patch.set_fields.push("summary".to_string());
                patch.summary = Some(packet.summary.clone());
            }
            if args.aliases.is_some() {
                patch.set_fields.push("aliases".to_string());
                patch.aliases = Some(packet.aliases.clone());
            }
            if args.core_members.is_some() {
                patch.set_fields.push("coreMembers".to_string());
                patch.core_members = Some(packet.core_members.clone());
                patch.core_member_lineages = Some(packet.core_member_lineages.clone());
            }
            if args.supporting_members.is_some() {
                patch.set_fields.push("supportingMembers".to_string());
                patch.supporting_members = Some(packet.supporting_members.clone());
                patch.supporting_member_lineages = Some(packet.supporting_member_lineages.clone());
            }
            if args.likely_tests.is_some() {
                patch.set_fields.push("likelyTests".to_string());
                patch.likely_tests = Some(packet.likely_tests.clone());
                patch.likely_test_lineages = Some(packet.likely_test_lineages.clone());
            }
            if args.evidence.is_some() {
                patch.set_fields.push("evidence".to_string());
                patch.evidence = Some(packet.evidence.clone());
            }
            match parse_sparse_patch(args.risk_hint.clone(), "riskHint")? {
                SparsePatch::Keep => {}
                SparsePatch::Set(_) => {
                    patch.set_fields.push("riskHint".to_string());
                    patch.risk_hint = packet.risk_hint.clone();
                }
                SparsePatch::Clear => patch.cleared_fields.push("riskHint".to_string()),
            }
            if args.confidence.is_some() {
                patch.set_fields.push("confidence".to_string());
                patch.confidence = Some(packet.confidence);
            }
            if args.decode_lenses.is_some() {
                patch.set_fields.push("decodeLenses".to_string());
                patch.decode_lenses = Some(packet.decode_lenses.clone());
            }
            if args.scope.is_some() {
                patch.set_fields.push("scope".to_string());
                patch.scope = Some(packet.scope);
            }
            if args.supersedes.is_some() {
                patch.set_fields.push("supersedes".to_string());
                patch.supersedes = Some(
                    packet
                        .publication
                        .as_ref()
                        .map(|publication| publication.supersedes.clone())
                        .unwrap_or_default(),
                );
            }
        }
        ConceptMutationOperationInput::Retire => {
            if args.retirement_reason.is_some() {
                patch.set_fields.push("retirementReason".to_string());
                patch.retirement_reason = packet
                    .publication
                    .as_ref()
                    .and_then(|publication| publication.retirement_reason.clone());
            }
        }
    }
    if patch.set_fields.is_empty() && patch.cleared_fields.is_empty() {
        Ok(None)
    } else {
        Ok(Some(patch))
    }
}

fn contract_event_patch(
    args: &PrismContractMutationArgs,
    operation: &ContractMutationOperationInput,
    packet: &ContractPacket,
) -> Result<Option<ContractEventPatch>> {
    let mut patch = ContractEventPatch::default();
    match operation {
        ContractMutationOperationInput::Promote => return Ok(None),
        ContractMutationOperationInput::Update => {
            if args.name.is_some() {
                patch.set_fields.push("name".to_string());
                patch.name = Some(packet.name.clone());
            }
            if args.summary.is_some() {
                patch.set_fields.push("summary".to_string());
                patch.summary = Some(packet.summary.clone());
            }
            if args.aliases.is_some() {
                patch.set_fields.push("aliases".to_string());
                patch.aliases = Some(packet.aliases.clone());
            }
            if args.kind.is_some() {
                patch.set_fields.push("kind".to_string());
                patch.kind = Some(packet.kind);
            }
            if args.subject.is_some() {
                patch.set_fields.push("subject".to_string());
                patch.subject = Some(packet.subject.clone());
            }
            if args.guarantees.is_some() {
                patch.set_fields.push("guarantees".to_string());
                patch.guarantees = Some(packet.guarantees.clone());
            }
            if args.assumptions.is_some() {
                patch.set_fields.push("assumptions".to_string());
                patch.assumptions = Some(packet.assumptions.clone());
            }
            if args.consumers.is_some() {
                patch.set_fields.push("consumers".to_string());
                patch.consumers = Some(packet.consumers.clone());
            }
            if args.validations.is_some() {
                patch.set_fields.push("validations".to_string());
                patch.validations = Some(packet.validations.clone());
            }
            if args.stability.is_some() {
                patch.set_fields.push("stability".to_string());
                patch.stability = Some(packet.stability);
            }
            if args.compatibility.is_some() {
                patch.set_fields.push("compatibility".to_string());
                patch.compatibility = Some(packet.compatibility.clone());
            }
            if args.evidence.is_some() {
                patch.set_fields.push("evidence".to_string());
                patch.evidence = Some(packet.evidence.clone());
            }
            if args.status.is_some() {
                patch.set_fields.push("status".to_string());
                patch.status = Some(packet.status);
            }
            if args.scope.is_some() {
                patch.set_fields.push("scope".to_string());
                patch.scope = Some(packet.scope);
            }
            if args.supersedes.is_some() {
                patch.set_fields.push("supersedes".to_string());
                patch.supersedes = Some(
                    packet
                        .publication
                        .as_ref()
                        .map(|publication| publication.supersedes.clone())
                        .unwrap_or_default(),
                );
            }
        }
        ContractMutationOperationInput::Retire => {
            patch.set_fields.push("status".to_string());
            patch.status = Some(packet.status);
            if args.retirement_reason.is_some() {
                patch.set_fields.push("retirementReason".to_string());
                patch.retirement_reason = args.retirement_reason.clone();
            }
        }
        ContractMutationOperationInput::AttachEvidence => {
            patch.set_fields.push("evidence".to_string());
            patch.evidence = Some(packet.evidence.clone());
        }
        ContractMutationOperationInput::AttachValidation => {
            patch.set_fields.push("validations".to_string());
            patch.validations = Some(packet.validations.clone());
        }
        ContractMutationOperationInput::RecordConsumer => {
            patch.set_fields.push("consumers".to_string());
            patch.consumers = Some(packet.consumers.clone());
        }
        ContractMutationOperationInput::SetStatus => {
            patch.set_fields.push("status".to_string());
            patch.status = Some(packet.status);
            if args.retirement_reason.is_some() {
                patch.set_fields.push("retirementReason".to_string());
                patch.retirement_reason = args.retirement_reason.clone();
            }
        }
    }
    if patch.set_fields.is_empty() && patch.cleared_fields.is_empty() {
        Ok(None)
    } else {
        Ok(Some(patch))
    }
}

fn required_contract_handle(handle: Option<&str>, message: &str) -> Result<String> {
    handle
        .map(|value| normalize_contract_handle(Some(value), value))
        .ok_or_else(|| anyhow!("{message}"))
}

fn current_contract(prism: &Prism, handle: &str) -> Result<ContractPacket> {
    prism
        .contract_by_handle(handle)
        .ok_or_else(|| anyhow!("no contract packet matched `{handle}`"))
}

fn normalize_contract_handle(handle: Option<&str>, name: &str) -> String {
    handle
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| canonical_contract_handle(value.trim_start_matches("contract://")))
        .unwrap_or_else(|| canonical_contract_handle(name))
}

fn normalize_contract_handles(handles: Vec<String>) -> Vec<String> {
    let mut normalized = sanitize_strings(handles)
        .into_iter()
        .map(|handle| canonical_contract_handle(handle.trim_start_matches("contract://")))
        .collect::<Vec<_>>();
    normalized.sort();
    normalized.dedup();
    normalized
}

fn update_contract_publication(
    publication: Option<ConceptPublication>,
    scope: ConceptScope,
    status: ContractStatus,
    recorded_at: u64,
    retirement_reason: Option<String>,
) -> Option<ConceptPublication> {
    if scope != ConceptScope::Repo && status != ContractStatus::Retired {
        return None;
    }
    let mut publication = publication.unwrap_or_default();
    if publication.published_at == 0 {
        publication.published_at = recorded_at;
    }
    publication.last_reviewed_at = Some(recorded_at);
    if status == ContractStatus::Retired {
        publication.status = ConceptPublicationStatus::Retired;
        publication.retired_at = Some(recorded_at);
        if retirement_reason.is_some() {
            publication.retirement_reason = retirement_reason;
        } else if publication.retirement_reason.is_none() {
            publication.retirement_reason = Some("retired".to_string());
        }
    } else {
        publication.status = ConceptPublicationStatus::Active;
        publication.retired_at = None;
        publication.retirement_reason = None;
    }
    Some(publication)
}

fn origin_for_scope(scope: ConceptScope) -> &'static str {
    match scope {
        ConceptScope::Local => "local_mutation",
        ConceptScope::Session => "session_mutation",
        ConceptScope::Repo => "repo_mutation",
    }
}

fn convert_contract_kind(kind: ContractKindInput) -> ContractKind {
    match kind {
        ContractKindInput::Interface => ContractKind::Interface,
        ContractKindInput::Behavioral => ContractKind::Behavioral,
        ContractKindInput::DataShape => ContractKind::DataShape,
        ContractKindInput::DependencyBoundary => ContractKind::DependencyBoundary,
        ContractKindInput::Lifecycle => ContractKind::Lifecycle,
        ContractKindInput::Protocol => ContractKind::Protocol,
        ContractKindInput::Operational => ContractKind::Operational,
    }
}

fn convert_contract_status(status: ContractStatusInput) -> ContractStatus {
    match status {
        ContractStatusInput::Candidate => ContractStatus::Candidate,
        ContractStatusInput::Active => ContractStatus::Active,
        ContractStatusInput::Deprecated => ContractStatus::Deprecated,
        ContractStatusInput::Retired => ContractStatus::Retired,
    }
}

fn convert_contract_stability(stability: ContractStabilityInput) -> ContractStability {
    match stability {
        ContractStabilityInput::Experimental => ContractStability::Experimental,
        ContractStabilityInput::Internal => ContractStability::Internal,
        ContractStabilityInput::Public => ContractStability::Public,
        ContractStabilityInput::Deprecated => ContractStability::Deprecated,
        ContractStabilityInput::Migrating => ContractStability::Migrating,
    }
}

fn convert_contract_guarantee_strength(
    strength: ContractGuaranteeStrengthInput,
) -> ContractGuaranteeStrength {
    match strength {
        ContractGuaranteeStrengthInput::Hard => ContractGuaranteeStrength::Hard,
        ContractGuaranteeStrengthInput::Soft => ContractGuaranteeStrength::Soft,
        ContractGuaranteeStrengthInput::Conditional => ContractGuaranteeStrength::Conditional,
    }
}

fn convert_contract_target(
    prism: &Prism,
    workspace_root: Option<&std::path::Path>,
    target: ContractTargetInput,
) -> Result<ContractTarget> {
    Ok(ContractTarget {
        anchors: convert_anchors(prism, workspace_root, target.anchors.unwrap_or_default())?,
        concept_handles: normalize_concept_handles(target.concept_handles.unwrap_or_default()),
    })
}

fn convert_contract_targets(
    prism: &Prism,
    workspace_root: Option<&std::path::Path>,
    targets: Option<Vec<ContractTargetInput>>,
) -> Result<Vec<ContractTarget>> {
    targets
        .unwrap_or_default()
        .into_iter()
        .map(|target| convert_contract_target(prism, workspace_root, target))
        .collect()
}

fn convert_contract_guarantees(
    guarantees: Vec<ContractGuaranteeInput>,
) -> Result<Vec<ContractGuarantee>> {
    let guarantees = guarantees
        .into_iter()
        .map(|guarantee| {
            let statement = guarantee.statement.trim().to_string();
            if statement.is_empty() {
                return Err(anyhow!("contract guarantees require non-empty statements"));
            }
            Ok(ContractGuarantee {
                id: guarantee
                    .id
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
                    .unwrap_or_default(),
                statement,
                scope: guarantee
                    .scope
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty()),
                strength: guarantee.strength.map(convert_contract_guarantee_strength),
                evidence_refs: sanitize_strings(guarantee.evidence_refs.unwrap_or_default()),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let guarantees = normalize_contract_guarantees(guarantees);
    if guarantees.is_empty() {
        return Err(anyhow!("contract guarantees cannot be empty"));
    }
    Ok(guarantees)
}

fn normalize_contract_guarantees(guarantees: Vec<ContractGuarantee>) -> Vec<ContractGuarantee> {
    let mut seen = std::collections::HashMap::<String, usize>::new();
    guarantees
        .into_iter()
        .map(|mut guarantee| {
            let base = normalize_contract_guarantee_id(if guarantee.id.trim().is_empty() {
                &guarantee.statement
            } else {
                &guarantee.id
            });
            let counter = seen.entry(base.clone()).or_insert(0);
            *counter += 1;
            guarantee.id = if *counter == 1 {
                base
            } else {
                format!("{base}_{}", *counter)
            };
            guarantee
        })
        .collect()
}

fn normalize_contract_guarantee_id(value: &str) -> String {
    let mut slug = String::new();
    let mut last_was_sep = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_was_sep = false;
        } else if !last_was_sep && !slug.is_empty() {
            slug.push('_');
            last_was_sep = true;
        }
    }
    while slug.ends_with('_') {
        slug.pop();
    }
    if slug.is_empty() {
        "guarantee".to_string()
    } else {
        slug
    }
}

fn convert_contract_validations(
    prism: &Prism,
    workspace_root: Option<&std::path::Path>,
    validations: Option<Vec<ContractValidationInput>>,
) -> Result<Vec<ContractValidation>> {
    validations
        .unwrap_or_default()
        .into_iter()
        .map(|validation| {
            let id = validation.id.trim().to_string();
            if id.is_empty() {
                return Err(anyhow!("contract validations require non-empty ids"));
            }
            Ok(ContractValidation {
                id,
                summary: validation
                    .summary
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty()),
                anchors: convert_anchors(
                    prism,
                    workspace_root,
                    validation.anchors.unwrap_or_default(),
                )?,
            })
        })
        .collect()
}

fn convert_contract_compatibility(
    compatibility: ContractCompatibilityInput,
) -> ContractCompatibility {
    ContractCompatibility {
        compatible: sanitize_strings(compatibility.compatible.unwrap_or_default()),
        additive: sanitize_strings(compatibility.additive.unwrap_or_default()),
        risky: sanitize_strings(compatibility.risky.unwrap_or_default()),
        breaking: sanitize_strings(compatibility.breaking.unwrap_or_default()),
        migrating: sanitize_strings(compatibility.migrating.unwrap_or_default()),
    }
}

fn merge_unique_strings(mut current: Vec<String>, additions: Vec<String>) -> Vec<String> {
    current.extend(additions);
    current = sanitize_strings(current);
    current.sort();
    current.dedup();
    current
}

fn merge_contract_targets(
    current: Vec<ContractTarget>,
    additions: Vec<ContractTarget>,
) -> Vec<ContractTarget> {
    let mut merged = current;
    for target in additions {
        if !merged.iter().any(|existing| existing == &target) {
            merged.push(target);
        }
    }
    merged
}

fn merge_contract_validations(
    current: Vec<ContractValidation>,
    additions: Vec<ContractValidation>,
) -> Vec<ContractValidation> {
    let mut merged = current;
    for validation in additions {
        if let Some(existing) = merged.iter_mut().find(|item| item.id == validation.id) {
            *existing = validation;
        } else {
            merged.push(validation);
        }
    }
    merged.sort_by(|left, right| left.id.cmp(&right.id));
    merged
}

fn build_updated_concept_packet(
    prism: &Prism,
    task_id: &TaskId,
    recorded_at: u64,
    args: PrismConceptMutationArgs,
) -> Result<ConceptPacket> {
    let handle = args
        .handle
        .as_deref()
        .map(|value| normalize_concept_handle(Some(value), value))
        .ok_or_else(|| anyhow!("concept update requires handle"))?;
    let mut packet = prism
        .concept_by_handle(&handle)
        .ok_or_else(|| anyhow!("no concept packet matched `{handle}`"))?;
    let mut changed = false;
    if let Some(scope) = args.scope.clone().map(convert_concept_scope) {
        packet.scope = scope;
        changed = true;
    }
    if let Some(canonical_name) = args
        .canonical_name
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        packet.canonical_name = canonical_name;
        changed = true;
    }
    if let Some(summary) = args
        .summary
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        packet.summary = summary;
        changed = true;
    }
    if let Some(aliases) = args.aliases {
        packet.aliases = sanitize_strings(aliases);
        changed = true;
    }
    if let Some(core_members) = args.core_members {
        packet.core_members = convert_concept_nodes(prism, core_members, "coreMembers")?;
        packet.core_member_lineages = concept_member_lineages(prism, &packet.core_members);
        changed = true;
    }
    if let Some(supporting_members) = args.supporting_members {
        packet.supporting_members =
            convert_concept_nodes(prism, supporting_members, "supportingMembers")?;
        packet.supporting_member_lineages =
            concept_member_lineages(prism, &packet.supporting_members);
        changed = true;
    }
    if let Some(likely_tests) = args.likely_tests {
        packet.likely_tests = convert_concept_nodes(prism, likely_tests, "likelyTests")?;
        packet.likely_test_lineages = concept_member_lineages(prism, &packet.likely_tests);
        changed = true;
    }
    if let Some(evidence) = args.evidence {
        packet.evidence = sanitize_strings(evidence);
        changed = true;
    }
    match parse_sparse_patch(args.risk_hint, "riskHint")? {
        SparsePatch::Keep => {}
        SparsePatch::Set(value) => {
            let risk_hint = value.trim().to_string();
            if risk_hint.is_empty() {
                return Err(anyhow!("concept riskHint cannot be empty"));
            }
            packet.risk_hint = Some(risk_hint);
            changed = true;
        }
        SparsePatch::Clear => {
            packet.risk_hint = None;
            changed = true;
        }
    }
    if let Some(confidence) = args.confidence {
        packet.confidence = confidence.clamp(0.0, 1.0);
        changed = true;
    }
    if let Some(decode_lenses) = args.decode_lenses {
        packet.decode_lenses = convert_concept_lenses(Some(decode_lenses));
        changed = true;
    }
    if let Some(supersedes) = args.supersedes {
        let publication = packet
            .publication
            .get_or_insert_with(|| ConceptPublication {
                published_at: recorded_at,
                ..ConceptPublication::default()
            });
        publication.supersedes = normalize_concept_handles(supersedes);
        changed = true;
    }
    if !changed {
        return Err(anyhow!(
            "concept update requires at least one field to change"
        ));
    }
    if packet.provenance == ConceptProvenance::default() {
        packet.provenance = ConceptProvenance {
            origin: match packet.scope {
                ConceptScope::Local => "local_mutation".to_string(),
                ConceptScope::Session => "session_mutation".to_string(),
                ConceptScope::Repo => "repo_mutation".to_string(),
            },
            kind: "manual_concept_update".to_string(),
            task_id: Some(task_id.0.to_string()),
        };
    }
    if packet.scope == ConceptScope::Repo {
        let publication = packet
            .publication
            .get_or_insert_with(|| ConceptPublication {
                published_at: recorded_at,
                ..ConceptPublication::default()
            });
        if publication.published_at == 0 {
            publication.published_at = recorded_at;
        }
        publication.last_reviewed_at = Some(recorded_at);
        publication.status = ConceptPublicationStatus::Active;
        publication.retired_at = None;
        publication.retirement_reason = None;
    } else {
        packet.publication = None;
    }
    validate_concept_packet(&packet)?;
    Ok(packet)
}

fn build_retired_concept_packet(
    prism: &Prism,
    task_id: &TaskId,
    recorded_at: u64,
    args: PrismConceptMutationArgs,
) -> Result<ConceptPacket> {
    let handle = args
        .handle
        .as_deref()
        .map(|value| normalize_concept_handle(Some(value), value))
        .ok_or_else(|| anyhow!("concept retire requires handle"))?;
    let retirement_reason = args
        .retirement_reason
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("concept retire requires retirementReason"))?;
    let mut packet = prism
        .concept_by_handle(&handle)
        .ok_or_else(|| anyhow!("no concept packet matched `{handle}`"))?;
    if let Some(scope) = args.scope.clone().map(convert_concept_scope) {
        packet.scope = scope;
    }
    if packet.provenance == ConceptProvenance::default() {
        packet.provenance = ConceptProvenance {
            origin: match packet.scope {
                ConceptScope::Local => "local_mutation".to_string(),
                ConceptScope::Session => "session_mutation".to_string(),
                ConceptScope::Repo => "repo_mutation".to_string(),
            },
            kind: "manual_concept_retire".to_string(),
            task_id: Some(task_id.0.to_string()),
        };
    }
    let publication = packet
        .publication
        .get_or_insert_with(|| ConceptPublication {
            published_at: recorded_at,
            ..ConceptPublication::default()
        });
    if publication.published_at == 0 {
        publication.published_at = recorded_at;
    }
    if let Some(supersedes) = args.supersedes {
        publication.supersedes = normalize_concept_handles(supersedes);
    }
    publication.last_reviewed_at = Some(recorded_at);
    publication.status = ConceptPublicationStatus::Retired;
    publication.retired_at = Some(recorded_at);
    publication.retirement_reason = Some(retirement_reason);
    validate_concept_packet(&packet)?;
    Ok(packet)
}

fn convert_optional_concept_nodes(
    prism: &Prism,
    value: Option<Vec<crate::NodeIdInput>>,
    field: &str,
) -> Result<Vec<prism_ir::NodeId>> {
    value
        .map(|nodes| convert_concept_nodes(prism, nodes, field))
        .transpose()
        .map(Option::unwrap_or_default)
}

fn convert_concept_nodes(
    prism: &Prism,
    nodes: Vec<crate::NodeIdInput>,
    field: &str,
) -> Result<Vec<prism_ir::NodeId>> {
    let mut converted = Vec::new();
    for node in nodes {
        let node_id = convert_node_id(node)?;
        if prism.graph().node(&node_id).is_none() {
            return Err(anyhow!(
                "concept `{field}` references unknown node `{}`",
                node_id.path
            ));
        }
        if !converted.iter().any(|candidate| candidate == &node_id) {
            converted.push(node_id);
        }
    }
    Ok(converted)
}

fn convert_concept_lenses(
    value: Option<Vec<PrismConceptLensInput>>,
) -> Vec<prism_query::ConceptDecodeLens> {
    value
        .unwrap_or_else(|| {
            vec![
                PrismConceptLensInput::Open,
                PrismConceptLensInput::Workset,
                PrismConceptLensInput::Validation,
                PrismConceptLensInput::Timeline,
                PrismConceptLensInput::Memory,
            ]
        })
        .into_iter()
        .map(|lens| match lens {
            PrismConceptLensInput::Open => prism_query::ConceptDecodeLens::Open,
            PrismConceptLensInput::Workset => prism_query::ConceptDecodeLens::Workset,
            PrismConceptLensInput::Validation => prism_query::ConceptDecodeLens::Validation,
            PrismConceptLensInput::Timeline => prism_query::ConceptDecodeLens::Timeline,
            PrismConceptLensInput::Memory => prism_query::ConceptDecodeLens::Memory,
        })
        .collect()
}

fn sanitize_strings(values: Vec<String>) -> Vec<String> {
    let mut sanitized = Vec::new();
    for value in values {
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        if !sanitized
            .iter()
            .any(|candidate: &String| candidate == value)
        {
            sanitized.push(value.to_string());
        }
    }
    sanitized
}

fn normalize_concept_handles(values: Vec<String>) -> Vec<String> {
    sanitize_strings(values)
        .into_iter()
        .map(|value| canonical_concept_handle(value.trim_start_matches("concept://")))
        .collect()
}

fn normalize_concept_handle(handle: Option<&str>, canonical_name: &str) -> String {
    match handle.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => canonical_concept_handle(value.trim_start_matches("concept://")),
        None => canonical_concept_handle(canonical_name),
    }
}

fn concept_member_lineages(
    prism: &Prism,
    members: &[prism_ir::NodeId],
) -> Vec<Option<prism_ir::LineageId>> {
    members
        .iter()
        .map(|member| prism.lineage_of(member))
        .collect()
}

fn validate_concept_packet(packet: &ConceptPacket) -> Result<()> {
    if packet.handle.trim().is_empty() {
        return Err(anyhow!("concept handle cannot be empty"));
    }
    if packet.canonical_name.trim().is_empty() {
        return Err(anyhow!("concept canonical name cannot be empty"));
    }
    if packet.summary.trim().is_empty() {
        return Err(anyhow!("concept summary cannot be empty"));
    }
    let min_core_members = if packet.scope == ConceptScope::Repo {
        2
    } else {
        1
    };
    if packet.core_members.len() < min_core_members {
        return Err(anyhow!(
            "concept coreMembers must contain at least {min_core_members} central member(s)"
        ));
    }
    if packet.core_members.len() > 5 {
        return Err(anyhow!(
            "concept coreMembers cannot contain more than 5 members"
        ));
    }
    if packet.evidence.is_empty() {
        return Err(anyhow!("concept evidence cannot be empty"));
    }
    let min_confidence = if packet.scope == ConceptScope::Repo {
        0.7
    } else {
        0.5
    };
    if packet.confidence < min_confidence {
        return Err(anyhow!(
            "concept confidence must be at least {min_confidence}"
        ));
    }
    if packet.decode_lenses.is_empty() {
        return Err(anyhow!("concept decodeLenses cannot be empty"));
    }
    if packet.scope == ConceptScope::Repo {
        let Some(publication) = packet.publication.as_ref() else {
            return Err(anyhow!(
                "repo-published concept packet must include publication metadata"
            ));
        };
        if publication.published_at == 0 {
            return Err(anyhow!(
                "repo-published concept publication metadata must include publishedAt"
            ));
        }
        if publication.status == ConceptPublicationStatus::Retired
            && publication
                .retirement_reason
                .as_deref()
                .unwrap_or("")
                .trim()
                .is_empty()
        {
            return Err(anyhow!(
                "retired concept publication metadata must include retirementReason"
            ));
        }
    } else if packet
        .publication
        .as_ref()
        .is_some_and(|publication| publication.status == ConceptPublicationStatus::Retired)
        && packet
            .publication
            .as_ref()
            .and_then(|publication| publication.retirement_reason.as_deref())
            .unwrap_or("")
            .trim()
            .is_empty()
    {
        return Err(anyhow!(
            "retired concept publication metadata must include retirementReason"
        ));
    }
    if packet.provenance == ConceptProvenance::default() {
        return Err(anyhow!("concept packet must include provenance metadata"));
    }
    Ok(())
}

fn validate_contract_packet(packet: &ContractPacket) -> Result<()> {
    if packet.handle.trim().is_empty() {
        return Err(anyhow!("contract handle cannot be empty"));
    }
    if packet.name.trim().is_empty() {
        return Err(anyhow!("contract name cannot be empty"));
    }
    if packet.summary.trim().is_empty() {
        return Err(anyhow!("contract summary cannot be empty"));
    }
    if packet.guarantees.is_empty() {
        return Err(anyhow!("contract guarantees cannot be empty"));
    }
    if packet
        .guarantees
        .iter()
        .any(|guarantee| guarantee.statement.trim().is_empty() || guarantee.id.trim().is_empty())
    {
        return Err(anyhow!(
            "contract guarantees must contain non-empty ids and statements"
        ));
    }
    let unique_guarantee_ids = packet
        .guarantees
        .iter()
        .map(|guarantee| guarantee.id.to_ascii_lowercase())
        .collect::<std::collections::HashSet<_>>();
    if unique_guarantee_ids.len() != packet.guarantees.len() {
        return Err(anyhow!(
            "contract guarantee ids must be unique within a packet"
        ));
    }
    if packet.subject.anchors.is_empty() && packet.subject.concept_handles.is_empty() {
        return Err(anyhow!(
            "contract subject must include at least one anchor or concept handle"
        ));
    }
    if packet.evidence.is_empty() {
        return Err(anyhow!("contract evidence cannot be empty"));
    }
    if packet.scope == ConceptScope::Repo {
        let Some(publication) = packet.publication.as_ref() else {
            return Err(anyhow!(
                "repo-published contract packet must include publication metadata"
            ));
        };
        if publication.published_at == 0 {
            return Err(anyhow!(
                "repo-published contract publication metadata must include publishedAt"
            ));
        }
    }
    if packet.status == ContractStatus::Retired
        && packet
            .publication
            .as_ref()
            .and_then(|publication| publication.retirement_reason.as_deref())
            .unwrap_or("")
            .trim()
            .is_empty()
    {
        return Err(anyhow!(
            "retired contract publication metadata must include retirementReason"
        ));
    }
    if packet.provenance == ConceptProvenance::default() {
        return Err(anyhow!("contract packet must include provenance metadata"));
    }
    Ok(())
}

fn convert_concept_scope(scope: crate::ConceptScopeInput) -> ConceptScope {
    match scope {
        crate::ConceptScopeInput::Local => ConceptScope::Local,
        crate::ConceptScopeInput::Session => ConceptScope::Session,
        crate::ConceptScopeInput::Repo => ConceptScope::Repo,
    }
}

fn convert_concept_relation_kind(kind: ConceptRelationKindInput) -> ConceptRelationKind {
    match kind {
        ConceptRelationKindInput::DependsOn => ConceptRelationKind::DependsOn,
        ConceptRelationKindInput::Specializes => ConceptRelationKind::Specializes,
        ConceptRelationKindInput::PartOf => ConceptRelationKind::PartOf,
        ConceptRelationKindInput::ValidatedBy => ConceptRelationKind::ValidatedBy,
        ConceptRelationKindInput::OftenUsedWith => ConceptRelationKind::OftenUsedWith,
        ConceptRelationKindInput::Supersedes => ConceptRelationKind::Supersedes,
        ConceptRelationKindInput::ConfusedWith => ConceptRelationKind::ConfusedWith,
    }
}

fn next_concept_event_id() -> String {
    new_prefixed_id("concept-event").to_string()
}

fn next_contract_event_id() -> String {
    new_prefixed_id("contract-event").to_string()
}

fn next_concept_relation_event_id() -> String {
    new_prefixed_id("concept-relation-event").to_string()
}

fn convert_validation_feedback_category(
    category: ValidationFeedbackCategoryInput,
) -> ValidationFeedbackCategory {
    match category {
        ValidationFeedbackCategoryInput::Structural => ValidationFeedbackCategory::Structural,
        ValidationFeedbackCategoryInput::Lineage => ValidationFeedbackCategory::Lineage,
        ValidationFeedbackCategoryInput::Memory => ValidationFeedbackCategory::Memory,
        ValidationFeedbackCategoryInput::Projection => ValidationFeedbackCategory::Projection,
        ValidationFeedbackCategoryInput::Coordination => ValidationFeedbackCategory::Coordination,
        ValidationFeedbackCategoryInput::Freshness => ValidationFeedbackCategory::Freshness,
        ValidationFeedbackCategoryInput::Other => ValidationFeedbackCategory::Other,
    }
}

fn convert_validation_feedback_verdict(
    verdict: ValidationFeedbackVerdictInput,
) -> ValidationFeedbackVerdict {
    match verdict {
        ValidationFeedbackVerdictInput::Wrong => ValidationFeedbackVerdict::Wrong,
        ValidationFeedbackVerdictInput::Stale => ValidationFeedbackVerdict::Stale,
        ValidationFeedbackVerdictInput::Noisy => ValidationFeedbackVerdict::Noisy,
        ValidationFeedbackVerdictInput::Helpful => ValidationFeedbackVerdict::Helpful,
        ValidationFeedbackVerdictInput::Mixed => ValidationFeedbackVerdict::Mixed,
    }
}
