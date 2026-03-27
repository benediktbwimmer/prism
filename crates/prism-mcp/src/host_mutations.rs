use anyhow::{anyhow, Result};
use prism_coordination::{
    HandoffAcceptInput, HandoffInput, PlanCreateInput, PlanUpdateInput, PolicyViolation,
    TaskCreateInput, TaskUpdateInput,
};
use prism_core::{ValidationFeedbackCategory, ValidationFeedbackRecord, ValidationFeedbackVerdict};
use prism_curator::{CuratorJobId, CuratorProposal, CuratorProposalDisposition};
use prism_ir::{
    AgentId, AnchorRef, ArtifactId, ClaimId, CoordinationTaskId, Edge, EdgeOrigin, EventActor,
    EventId, EventMeta, PlanId, TaskId,
};
use prism_js::TaskJournalView;
use prism_memory::{
    MemoryEntry, MemoryKind, MemoryModule, MemorySource, OutcomeEvent, OutcomeEvidence,
    OutcomeKind, OutcomeResult,
};
use prism_query::Prism;
use serde_json::{json, Value};

use crate::{
    artifact_view, claim_view, conflict_view, convert_acceptance, convert_anchors,
    convert_completion_context, convert_inferred_scope, convert_memory_kind, convert_memory_source,
    convert_node_id, convert_outcome_evidence, convert_outcome_kind, convert_outcome_result,
    convert_policy, coordination_task_view, curator_disposition_label, curator_job_status_label,
    curator_memory_metadata, curator_proposal, curator_proposal_state, curator_trigger_label,
    current_timestamp, manual_memory_metadata, parse_capability, parse_claim_mode,
    parse_coordination_task_status, parse_edge_kind, parse_plan_status, parse_review_verdict,
    plan_view, task_journal_memory_metadata, task_journal_view, ArtifactActionInput,
    ArtifactMutationResult, ArtifactProposePayload, ArtifactReviewPayload,
    ArtifactSupersedePayload, ClaimAcquirePayload, ClaimActionInput, ClaimMutationResult,
    ClaimReleasePayload, ClaimRenewPayload, CoordinationMutationKindInput,
    CoordinationMutationResult, CuratorJobView, CuratorProposalDecisionResult, EdgeMutationResult,
    EventMutationResult, HandoffAcceptPayload, MemoryMutationActionInput, MemoryMutationResult,
    MemoryStorePayload, MutationViolationView, PlanUpdatePayload, PrismArtifactArgs,
    PrismClaimArgs, PrismCoordinationArgs, PrismCuratorPromoteEdgeArgs,
    PrismCuratorPromoteMemoryArgs, PrismCuratorRejectProposalArgs, PrismFinishTaskArgs,
    PrismInferEdgeArgs, PrismMemoryArgs, PrismOutcomeArgs, PrismValidationFeedbackArgs, QueryHost,
    SessionState, TaskCreatePayload, TaskUpdatePayload, ValidationFeedbackCategoryInput,
    ValidationFeedbackMutationResult, ValidationFeedbackVerdictInput,
    DEFAULT_TASK_JOURNAL_EVENT_LIMIT, DEFAULT_TASK_JOURNAL_MEMORY_LIMIT,
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
    for event in prism.coordination().events().into_iter().skip(before_len) {
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
        description: String,
        tags: Vec<String>,
    ) -> Result<TaskId> {
        let task = session.start_task(&description, &tags);
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
            metadata: json!({ "tags": tags }),
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
        Ok(task)
    }

    #[allow(dead_code)]
    pub(crate) fn finish_task(
        &self,
        session: &SessionState,
        args: PrismFinishTaskArgs,
    ) -> Result<TaskClosureMutationResult> {
        self.refresh_workspace()?;
        self.close_task_without_refresh(session, args, TaskClosureDisposition::Completed)
    }

    #[allow(dead_code)]
    pub(crate) fn abandon_task(
        &self,
        session: &SessionState,
        args: PrismFinishTaskArgs,
    ) -> Result<TaskClosureMutationResult> {
        self.refresh_workspace()?;
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
        let replay = prism.resume_task(&task);
        if replay.events.is_empty() && metadata_override.is_none() {
            return Err(anyhow!("unknown task `{}`", task.0));
        }

        let mut anchors = replay
            .events
            .iter()
            .flat_map(|event| event.anchors.iter().cloned())
            .collect::<Vec<_>>();
        if let Some(explicit) = args.anchors {
            anchors.extend(convert_anchors(explicit)?);
        }
        let anchors = prism.anchors_for(&anchors);

        let mut entry = MemoryEntry::new(MemoryKind::Episodic, args.summary.clone());
        entry.anchors = anchors.clone();
        entry.source = MemorySource::Agent;
        entry.trust = disposition.trust();
        entry.metadata = task_journal_memory_metadata(Value::Null, &task, disposition.label());
        let memory_id = session.notes.store(entry)?;

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
            let event_id = workspace.append_outcome_with_auxiliary(
                event,
                Some(session.notes.snapshot()),
                None,
            )?;
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

        let journal = task_journal_view(
            session,
            self.current_prism().as_ref(),
            &task,
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
        self.refresh_workspace()?;
        self.store_outcome_without_refresh(session, args)
    }

    pub(crate) fn store_outcome_without_refresh(
        &self,
        session: &SessionState,
        args: PrismOutcomeArgs,
    ) -> Result<EventMutationResult> {
        let prism = self.current_prism();
        let anchors = prism.anchors_for(&convert_anchors(args.anchors)?);
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
        self.refresh_workspace()?;
        self.store_memory_without_refresh(session, args)
    }

    pub(crate) fn store_memory_without_refresh(
        &self,
        session: &SessionState,
        args: PrismMemoryArgs,
    ) -> Result<MemoryMutationResult> {
        let prism = self.current_prism();
        let task_id = session.task_for_mutation(args.task_id.map(TaskId::new));
        let payload = match args.action {
            MemoryMutationActionInput::Store => {
                serde_json::from_value::<MemoryStorePayload>(args.payload)?
            }
        };
        let anchors = prism.anchors_for(&convert_anchors(payload.anchors)?);
        let kind = convert_memory_kind(payload.kind);
        let mut entry = MemoryEntry::new(kind, payload.content);
        entry.anchors = anchors;
        entry.source = payload
            .source
            .map(convert_memory_source)
            .unwrap_or(MemorySource::Agent);
        entry.trust = payload.trust.unwrap_or(0.5).clamp(0.0, 1.0);
        entry.metadata = manual_memory_metadata(payload.metadata.unwrap_or(Value::Null), &task_id);
        let note_anchors = entry.anchors.clone();
        let note_content = entry.content.clone();
        let memory_id = session.notes.store(entry)?;
        if kind == MemoryKind::Episodic {
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
                let _ = workspace.append_outcome_with_auxiliary(
                    note_event,
                    Some(session.notes.snapshot()),
                    None,
                )?;
                self.sync_workspace_revision(workspace)?;
                self.sync_episodic_revision(workspace)?;
            } else {
                prism.apply_outcome_event_to_projections(&note_event);
                let _ = prism.outcome_memory().store_event(note_event)?;
                self.persist_outcomes()?;
                self.persist_notes()?;
            }
        } else {
            self.persist_notes()?;
        }
        Ok(MemoryMutationResult {
            memory_id: memory_id.0,
            task_id: task_id.0.to_string(),
        })
    }

    #[allow(dead_code)]
    pub(crate) fn store_validation_feedback(
        &self,
        session: &SessionState,
        args: PrismValidationFeedbackArgs,
    ) -> Result<ValidationFeedbackMutationResult> {
        self.refresh_workspace()?;
        self.store_validation_feedback_without_refresh(session, args)
    }

    pub(crate) fn store_validation_feedback_without_refresh(
        &self,
        session: &SessionState,
        args: PrismValidationFeedbackArgs,
    ) -> Result<ValidationFeedbackMutationResult> {
        let prism = self.current_prism();
        let task_id = session.task_for_mutation(args.task_id.map(TaskId::new));
        let anchors = prism.anchors_for(&convert_anchors(args.anchors)?);
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
            self.persist_inferred_edges()?;
        }
        Ok(EdgeMutationResult {
            edge_id: id.0,
            task_id: task.0.to_string(),
        })
    }

    pub(crate) fn store_coordination(
        &self,
        session: &SessionState,
        args: PrismCoordinationArgs,
    ) -> Result<CoordinationMutationResult> {
        self.ensure_tool_enabled("prism_coordination", "coordination workflow mutations")?;
        self.refresh_workspace()?;
        let prism = self.current_prism();
        let before_events = prism.coordination().events().len();
        let task = session.task_for_mutation(args.task_id.clone().map(TaskId::new));
        let event_id = session.next_event_id("coordination");
        let meta = EventMeta {
            id: event_id.clone(),
            ts: current_timestamp(),
            actor: EventActor::Agent,
            correlation: Some(task),
            causation: None,
        };
        let state = if let Some(workspace) = &self.workspace {
            match workspace.mutate_coordination(|prism| {
                self.apply_coordination_mutation(session, prism, args, meta.clone())
            }) {
                Ok(state) => {
                    self.sync_coordination_revision(workspace)?;
                    state
                }
                Err(error) => {
                    let audit = coordination_audit_since(prism.as_ref(), before_events);
                    if audit.rejected && !audit.event_ids.is_empty() {
                        workspace.persist_current_coordination()?;
                        self.sync_coordination_revision(workspace)?;
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
        } else {
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
        self.refresh_workspace()?;
        let prism = self.current_prism();
        let before_events = prism.coordination().events().len();
        let task = session.task_for_mutation(args.task_id.clone().map(TaskId::new));
        let meta = EventMeta {
            id: session.next_event_id("coordination"),
            ts: current_timestamp(),
            actor: EventActor::Agent,
            correlation: Some(task),
            causation: None,
        };
        if let Some(workspace) = &self.workspace {
            match workspace.mutate_coordination(|prism| {
                self.apply_claim_mutation(session, prism, args, meta.clone())
            }) {
                Ok(mut result) => {
                    self.sync_coordination_revision(workspace)?;
                    let audit = coordination_audit_since(prism.as_ref(), before_events);
                    result.event_ids = audit.event_ids;
                    result.violations.extend(audit.violations);
                    Ok(result)
                }
                Err(error) => {
                    let audit = coordination_audit_since(prism.as_ref(), before_events);
                    if audit.rejected && !audit.event_ids.is_empty() {
                        workspace.persist_current_coordination()?;
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
        self.refresh_workspace()?;
        let prism = self.current_prism();
        let before_events = prism.coordination().events().len();
        let task = session.task_for_mutation(args.task_id.clone().map(TaskId::new));
        let meta = EventMeta {
            id: session.next_event_id("coordination"),
            ts: current_timestamp(),
            actor: EventActor::Agent,
            correlation: Some(task),
            causation: None,
        };
        if let Some(workspace) = &self.workspace {
            match workspace.mutate_coordination(|prism| {
                self.apply_artifact_mutation(prism, args, meta.clone())
            }) {
                Ok(mut result) => {
                    self.sync_coordination_revision(workspace)?;
                    let audit = coordination_audit_since(prism.as_ref(), before_events);
                    result.event_ids = audit.event_ids;
                    result.violations.extend(audit.violations);
                    Ok(result)
                }
                Err(error) => {
                    let audit = coordination_audit_since(prism.as_ref(), before_events);
                    if audit.rejected && !audit.event_ids.is_empty() {
                        workspace.persist_current_coordination()?;
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
        match args.kind {
            CoordinationMutationKindInput::PlanCreate => {
                let payload: crate::PlanCreatePayload = serde_json::from_value(args.payload)?;
                let (_, plan) = prism.coordination().create_plan(
                    meta,
                    PlanCreateInput {
                        goal: payload.goal,
                        status: payload
                            .status
                            .as_deref()
                            .map(parse_plan_status)
                            .transpose()?,
                        policy: convert_policy(payload.policy)?,
                    },
                )?;
                Ok(serde_json::to_value(plan_view(plan))?)
            }
            CoordinationMutationKindInput::PlanUpdate => {
                let payload: PlanUpdatePayload = serde_json::from_value(args.payload)?;
                let plan = prism.coordination().update_plan(
                    meta,
                    PlanUpdateInput {
                        plan_id: PlanId::new(payload.plan_id),
                        status: payload
                            .status
                            .as_deref()
                            .map(parse_plan_status)
                            .transpose()?,
                        goal: payload.goal,
                        policy: convert_policy(payload.policy)?,
                    },
                )?;
                Ok(serde_json::to_value(plan_view(plan))?)
            }
            CoordinationMutationKindInput::TaskCreate => {
                let payload: TaskCreatePayload = serde_json::from_value(args.payload)?;
                let (_, task) = prism.coordination().create_task(
                    meta,
                    TaskCreateInput {
                        plan_id: PlanId::new(payload.plan_id),
                        title: payload.title,
                        status: payload
                            .status
                            .as_deref()
                            .map(parse_coordination_task_status)
                            .transpose()?,
                        assignee: payload
                            .assignee
                            .map(AgentId::new)
                            .or_else(|| session.current_agent()),
                        session: Some(session.session_id()),
                        anchors: convert_anchors(payload.anchors.unwrap_or_default())?,
                        depends_on: payload
                            .depends_on
                            .unwrap_or_default()
                            .into_iter()
                            .map(CoordinationTaskId::new)
                            .collect(),
                        acceptance: convert_acceptance(payload.acceptance)?,
                        base_revision: prism.workspace_revision(),
                    },
                )?;
                Ok(serde_json::to_value(coordination_task_view(task))?)
            }
            CoordinationMutationKindInput::TaskUpdate => {
                let payload: TaskUpdatePayload = serde_json::from_value(args.payload)?;
                let task_id = CoordinationTaskId::new(payload.task_id.clone());
                let status = payload
                    .status
                    .as_deref()
                    .map(parse_coordination_task_status)
                    .transpose()?;
                let completion_context = convert_completion_context(payload.completion_context)
                    .or_else(|| {
                        status
                            .filter(|status| *status == prism_ir::CoordinationTaskStatus::Completed)
                            .and_then(|_| prism.task_risk(&task_id, meta.ts))
                            .map(|risk| prism_coordination::TaskCompletionContext {
                                risk_score: Some(risk.risk_score),
                                required_validations: risk.likely_validations,
                            })
                    });
                let task = prism.coordination().update_task(
                    meta,
                    TaskUpdateInput {
                        task_id,
                        status,
                        assignee: payload.assignee.map(|value| Some(AgentId::new(value))),
                        session: None,
                        title: payload.title,
                        anchors: payload.anchors.map(convert_anchors).transpose()?,
                        base_revision: Some(prism.workspace_revision()),
                        completion_context,
                    },
                    prism.workspace_revision(),
                    current_timestamp(),
                )?;
                Ok(serde_json::to_value(coordination_task_view(task))?)
            }
            CoordinationMutationKindInput::Handoff => {
                let payload: crate::HandoffPayload = serde_json::from_value(args.payload)?;
                let task = prism.coordination().handoff(
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
                let task = prism.coordination().accept_handoff(
                    meta,
                    HandoffAcceptInput {
                        task_id: CoordinationTaskId::new(payload.task_id),
                        agent: session_agent,
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
        match args.action {
            ClaimActionInput::Acquire => {
                let payload: ClaimAcquirePayload = serde_json::from_value(args.payload)?;
                let anchors = prism.coordination_scope_anchors(&convert_anchors(payload.anchors)?);
                let (claim_id, conflicts, state) = prism.coordination().acquire_claim(
                    meta,
                    session.session_id(),
                    prism_coordination::ClaimAcquireInput {
                        task_id: payload.coordination_task_id.map(CoordinationTaskId::new),
                        anchors,
                        capability: parse_capability(&payload.capability)?,
                        mode: payload.mode.as_deref().map(parse_claim_mode).transpose()?,
                        ttl_seconds: payload.ttl_seconds,
                        base_revision: prism.workspace_revision(),
                        current_revision: prism.workspace_revision(),
                        agent: payload
                            .agent
                            .map(AgentId::new)
                            .or_else(|| session.current_agent()),
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
                let claim = prism.coordination().renew_claim(
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
                let claim = prism.coordination().release_claim(
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
        match args.action {
            ArtifactActionInput::Propose => {
                let payload: ArtifactProposePayload = serde_json::from_value(args.payload)?;
                let task_id = CoordinationTaskId::new(payload.task_id.clone());
                let anchors = match payload.anchors {
                    Some(anchors) => convert_anchors(anchors)?,
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
                    if let Some(event) = prism.outcome_memory().event(event_id) {
                        if matches!(event.result, prism_memory::OutcomeResult::Success) {
                            inferred_validated_checks.extend(event.evidence.iter().filter_map(
                                |evidence| match evidence {
                                    OutcomeEvidence::Test { name, passed } if *passed => {
                                        Some(format!("test:{name}"))
                                    }
                                    OutcomeEvidence::Build { target, passed } if *passed => {
                                        Some(format!("build:{target}"))
                                    }
                                    _ => None,
                                },
                            ));
                        }
                    }
                }
                inferred_validated_checks.sort();
                inferred_validated_checks.dedup();
                let recipe = prism.task_validation_recipe(&task_id);
                let risk = prism.task_risk(&task_id, meta.ts);
                let (artifact_id, artifact) = prism.coordination().propose_artifact(
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
                let artifact = prism.coordination().supersede_artifact(
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
                let (review_id, _, artifact) = prism.coordination().review_artifact(
                    meta,
                    prism_coordination::ArtifactReviewInput {
                        artifact_id,
                        verdict: parse_review_verdict(&payload.verdict)?,
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
        self.refresh_workspace()?;
        let workspace = self
            .workspace
            .as_ref()
            .ok_or_else(|| anyhow!("curator mutations require a workspace-backed session"))?;
        let job_id = CuratorJobId(args.job_id.clone());
        let snapshot = workspace.curator_snapshot();
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
            self.persist_inferred_edges()?;
        }
        workspace.set_curator_proposal_state(
            &job_id,
            args.proposal_index,
            CuratorProposalDisposition::Applied,
            Some(task),
            args.note,
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
            proposal: serde_json::to_value(proposal)?,
            memory_id: None,
            edge_id: Some(edge_id.0),
        })
    }

    pub(crate) fn promote_curator_memory(
        &self,
        session: &SessionState,
        args: PrismCuratorPromoteMemoryArgs,
    ) -> Result<CuratorProposalDecisionResult> {
        self.refresh_workspace()?;
        let workspace = self
            .workspace
            .as_ref()
            .ok_or_else(|| anyhow!("curator mutations require a workspace-backed session"))?;
        let job_id = CuratorJobId(args.job_id.clone());
        let snapshot = workspace.curator_snapshot();
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
        let entry = match proposal {
            CuratorProposal::StructuralMemory(candidate) => {
                let mut entry = MemoryEntry::new(candidate.kind, candidate.content.clone());
                entry.anchors = prism.anchors_for(&candidate.anchors);
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
                entry
            }
            CuratorProposal::SemanticMemory(candidate) => {
                let mut entry = MemoryEntry::new(candidate.kind, candidate.content.clone());
                entry.anchors = prism.anchors_for(&candidate.anchors);
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
                entry
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
                        validation_checks: Vec::new(),
                        co_change_lineages: Vec::new(),
                    },
                };
                let mut entry =
                    MemoryEntry::new(MemoryKind::Semantic, candidate_memory.content.clone());
                entry.anchors = prism.anchors_for(&candidate.anchors);
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
                entry
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
                        validation_checks: candidate.checks.clone(),
                        co_change_lineages: Vec::new(),
                    },
                };
                let mut entry =
                    MemoryEntry::new(MemoryKind::Structural, candidate_memory.content.clone());
                entry.anchors = prism.anchors_for(&[AnchorRef::Node(candidate.target.clone())]);
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
                entry
            }
            CuratorProposal::InferredEdge(_) => {
                return Err(anyhow!(
                    "curator proposal {} for job `{}` is an inferred edge; use prism_mutate with action `curator_promote_edge`",
                    args.proposal_index,
                    args.job_id
                ));
            }
        };
        let memory_summary = entry.content.clone();
        let memory_anchors = entry.anchors.clone();
        let memory_id = session.notes.store(entry)?;
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
            let _ = workspace.append_outcome_with_auxiliary(
                note_event,
                Some(session.notes.snapshot()),
                None,
            )?;
            self.sync_workspace_revision(workspace)?;
            self.sync_episodic_revision(workspace)?;
        } else {
            prism.apply_outcome_event_to_projections(&note_event);
            let _ = prism.outcome_memory().store_event(note_event)?;
            self.persist_outcomes()?;
            self.persist_notes()?;
        }
        workspace.set_curator_proposal_state(
            &job_id,
            args.proposal_index,
            CuratorProposalDisposition::Applied,
            Some(task),
            args.note,
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
            proposal: serde_json::to_value(proposal)?,
            memory_id: Some(memory_id.0),
            edge_id: None,
        })
    }

    pub(crate) fn reject_curator_proposal(
        &self,
        session: &SessionState,
        args: PrismCuratorRejectProposalArgs,
    ) -> Result<CuratorProposalDecisionResult> {
        self.refresh_workspace()?;
        let workspace = self
            .workspace
            .as_ref()
            .ok_or_else(|| anyhow!("curator mutations require a workspace-backed session"))?;
        let job_id = CuratorJobId(args.job_id.clone());
        let snapshot = workspace.curator_snapshot();
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
        workspace.set_curator_proposal_state(
            &job_id,
            args.proposal_index,
            CuratorProposalDisposition::Rejected,
            Some(task),
            args.reason,
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
            proposal: serde_json::to_value(proposal)?,
            memory_id: None,
            edge_id: None,
        })
    }

    pub(crate) fn curator_jobs(&self, args: crate::CuratorJobsArgs) -> Result<Vec<CuratorJobView>> {
        self.refresh_workspace()?;
        let Some(workspace) = &self.workspace else {
            return Ok(Vec::new());
        };
        let mut jobs = workspace
            .curator_snapshot()
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

    pub(crate) fn curator_job(&self, job_id: &str) -> Result<Option<CuratorJobView>> {
        self.refresh_workspace()?;
        let Some(workspace) = &self.workspace else {
            return Ok(None);
        };
        workspace
            .curator_snapshot()
            .records
            .into_iter()
            .find(|record| record.id.0 == job_id)
            .map(crate::curator_job_view)
            .transpose()
    }
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
