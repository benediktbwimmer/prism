use anyhow::{anyhow, Result};
use prism_ir::{
    ArtifactStatus, ClaimId, ClaimMode, ConflictSeverity, CoordinationEventKind,
    CoordinationTaskId, CoordinationTaskStatus, EventId, EventMeta, PlanId, PlanStatus, ReviewId,
    ReviewVerdict, SessionId, Timestamp, WorkspaceRevision,
};
use serde_json::Value;

use crate::helpers::{
    dedupe_anchors, dedupe_conflicts, dedupe_event_ids, dedupe_ids, dedupe_strings,
    editor_capacity_conflicts, expire_claims_locked, missing_validations_for_artifact,
    normalize_acceptance, plan_policy_for_task, simulate_conflicts, validate_task_transition,
};
use crate::state::CoordinationStore;
use crate::types::{
    Artifact, ArtifactProposeInput, ArtifactReview, ArtifactReviewInput, ArtifactSupersedeInput,
    ClaimAcquireInput, CoordinationEvent, CoordinationTask, HandoffInput, Plan, PlanCreateInput,
    TaskUpdateInput, WorkClaim,
};

impl CoordinationStore {
    pub fn create_plan(&self, meta: EventMeta, input: PlanCreateInput) -> Result<(PlanId, Plan)> {
        let mut state = self
            .state
            .write()
            .expect("coordination store lock poisoned");
        state.next_plan += 1;
        let id = PlanId::new(format!("plan:{}", state.next_plan));
        let plan = Plan {
            id: id.clone(),
            goal: input.goal.clone(),
            status: PlanStatus::Active,
            policy: input.policy.unwrap_or_default(),
            root_tasks: Vec::new(),
        };
        state.plans.insert(id.clone(), plan.clone());
        state.events.push(CoordinationEvent {
            meta,
            kind: CoordinationEventKind::PlanCreated,
            summary: input.goal,
            plan: Some(id.clone()),
            task: None,
            claim: None,
            artifact: None,
            review: None,
            metadata: Value::Null,
        });
        Ok((id, plan))
    }

    pub fn create_task(
        &self,
        meta: EventMeta,
        input: crate::types::TaskCreateInput,
    ) -> Result<(CoordinationTaskId, CoordinationTask)> {
        let mut state = self
            .state
            .write()
            .expect("coordination store lock poisoned");
        if !state.plans.contains_key(&input.plan_id) {
            return Err(anyhow!("unknown plan `{}`", input.plan_id.0));
        }
        for dependency in &input.depends_on {
            let Some(task) = state.tasks.get(dependency) else {
                return Err(anyhow!("unknown dependency task `{}`", dependency.0));
            };
            if task.plan != input.plan_id {
                return Err(anyhow!(
                    "dependency task `{}` belongs to a different plan",
                    dependency.0
                ));
            }
        }
        state.next_task += 1;
        let id = CoordinationTaskId::new(format!("coord-task:{}", state.next_task));
        let is_root = input.depends_on.is_empty();
        let task = CoordinationTask {
            id: id.clone(),
            plan: input.plan_id.clone(),
            title: input.title.clone(),
            status: input.status.unwrap_or(CoordinationTaskStatus::Ready),
            assignee: input.assignee,
            session: input.session,
            anchors: dedupe_anchors(input.anchors),
            depends_on: dedupe_ids(input.depends_on),
            acceptance: normalize_acceptance(input.acceptance),
            base_revision: input.base_revision,
        };
        if is_root {
            let plan = state
                .plans
                .get_mut(&input.plan_id)
                .expect("plan validated above");
            plan.root_tasks.push(id.clone());
            plan.root_tasks = dedupe_ids(plan.root_tasks.clone());
        }
        state.tasks.insert(id.clone(), task.clone());
        state.events.push(CoordinationEvent {
            meta,
            kind: CoordinationEventKind::TaskCreated,
            summary: input.title,
            plan: Some(input.plan_id),
            task: Some(id.clone()),
            claim: None,
            artifact: None,
            review: None,
            metadata: Value::Null,
        });
        Ok((id, task))
    }

    pub fn update_task(
        &self,
        meta: EventMeta,
        input: TaskUpdateInput,
        current_revision: WorkspaceRevision,
        now: Timestamp,
    ) -> Result<CoordinationTask> {
        let completion_context = input.completion_context.clone();
        let mut state = self
            .state
            .write()
            .expect("coordination store lock poisoned");
        let previous = state
            .tasks
            .get(&input.task_id)
            .cloned()
            .ok_or_else(|| anyhow!("unknown coordination task `{}`", input.task_id.0))?;
        let stale_writes_enforced = state
            .plans
            .get(&previous.plan)
            .map(|plan| plan.policy.stale_after_graph_change)
            .unwrap_or(false);
        let task_snapshot;
        let status_changed;
        {
            let task = state
                .tasks
                .get_mut(&input.task_id)
                .expect("task validated above");
            if stale_writes_enforced
                && previous.base_revision.graph_version < current_revision.graph_version
                && input.base_revision.is_none()
            {
                return Err(anyhow!(
                    "coordination task `{}` is stale against graph version {}; provide an updated base revision before mutating it",
                    previous.id.0,
                    current_revision.graph_version
                ));
            }
            if let Some(base_revision) = &input.base_revision {
                if stale_writes_enforced
                    && base_revision.graph_version < current_revision.graph_version
                {
                    return Err(anyhow!(
                        "coordination task `{}` cannot use stale base revision {} when current revision is {}",
                        previous.id.0,
                        base_revision.graph_version,
                        current_revision.graph_version
                    ));
                }
            }
            status_changed = input
                .status
                .map(|status| status != previous.status)
                .unwrap_or(false);
            if let Some(status) = input.status {
                validate_task_transition(previous.status, status)?;
            }
            if matches!(
                previous.status,
                CoordinationTaskStatus::Completed | CoordinationTaskStatus::Abandoned
            ) && (input.title.is_some()
                || input.anchors.is_some()
                || input.assignee.is_some()
                || input.session.is_some())
            {
                return Err(anyhow!(
                    "terminal coordination task `{}` cannot be edited",
                    previous.id.0
                ));
            }
            if let Some(title) = input.title {
                task.title = title;
            }
            if let Some(status) = input.status {
                task.status = status;
            }
            if let Some(assignee) = input.assignee {
                task.assignee = assignee;
            }
            if let Some(session) = input.session {
                task.session = session;
            }
            if let Some(anchors) = input.anchors {
                task.anchors = dedupe_anchors(anchors);
            }
            if let Some(base_revision) = input.base_revision {
                task.base_revision = base_revision;
            }
            task_snapshot = task.clone();
        }
        let completion_blockers =
            self.completion_blockers_locked(&state, &task_snapshot, current_revision.clone(), now);
        let mut policy_blockers = if task_snapshot.status == CoordinationTaskStatus::Completed {
            self.completion_policy_blockers_locked(
                &state,
                &task_snapshot,
                current_revision,
                completion_context.as_ref(),
            )
        } else {
            Vec::new()
        };
        if task_snapshot.status == CoordinationTaskStatus::Completed
            && (!completion_blockers.is_empty() || !policy_blockers.is_empty())
        {
            let mut blockers = completion_blockers;
            blockers.append(&mut policy_blockers);
            return Err(anyhow!(
                "coordination task `{}` cannot complete: {}",
                task_snapshot.id.0,
                blockers
                    .iter()
                    .map(|blocker| blocker.summary.as_str())
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
        let task = task_snapshot;
        let kind = if previous.assignee != task.assignee {
            CoordinationEventKind::TaskAssigned
        } else if status_changed && task.status == CoordinationTaskStatus::Blocked {
            CoordinationEventKind::TaskBlocked
        } else if previous.status == CoordinationTaskStatus::Blocked && status_changed {
            CoordinationEventKind::TaskUnblocked
        } else {
            CoordinationEventKind::TaskStatusChanged
        };
        state.events.push(CoordinationEvent {
            meta,
            kind,
            summary: task.title.clone(),
            plan: Some(task.plan.clone()),
            task: Some(task.id.clone()),
            claim: None,
            artifact: None,
            review: None,
            metadata: Value::Null,
        });
        Ok(task)
    }

    pub fn handoff(
        &self,
        meta: EventMeta,
        input: HandoffInput,
        current_revision: WorkspaceRevision,
    ) -> Result<CoordinationTask> {
        let mut state = self
            .state
            .write()
            .expect("coordination store lock poisoned");
        let plan = {
            let task = state
                .tasks
                .get(&input.task_id)
                .ok_or_else(|| anyhow!("unknown coordination task `{}`", input.task_id.0))?;
            state.plans.get(&task.plan).cloned()
        };
        if input.base_revision.graph_version < current_revision.graph_version {
            return Err(anyhow!(
                "coordination task `{}` cannot hand off from stale base revision {} when current revision is {}",
                input.task_id.0,
                input.base_revision.graph_version,
                current_revision.graph_version
            ));
        }
        if plan
            .as_ref()
            .map(|plan| plan.policy.stale_after_graph_change)
            .unwrap_or(false)
        {
            let task = state
                .tasks
                .get(&input.task_id)
                .expect("task validated above");
            if task.base_revision.graph_version < current_revision.graph_version {
                return Err(anyhow!(
                    "coordination task `{}` is stale against graph version {} and cannot be handed off until refreshed",
                    input.task_id.0,
                    current_revision.graph_version
                ));
            }
        }
        let task = state
            .tasks
            .get_mut(&input.task_id)
            .ok_or_else(|| anyhow!("unknown coordination task `{}`", input.task_id.0))?;
        let target_agent = input.to_agent.clone();
        task.assignee = target_agent.clone();
        task.session = None;
        task.status = CoordinationTaskStatus::Ready;
        task.base_revision = input.base_revision.clone();
        let task = task.clone();
        state.events.push(CoordinationEvent {
            meta: meta.clone(),
            kind: CoordinationEventKind::HandoffRequested,
            summary: input.summary.clone(),
            plan: Some(task.plan.clone()),
            task: Some(task.id.clone()),
            claim: None,
            artifact: None,
            review: None,
            metadata: serde_json::json!({
                "to_agent": target_agent.map(|agent| agent.0.to_string())
            }),
        });
        state.events.push(CoordinationEvent {
            meta,
            kind: CoordinationEventKind::HandoffAccepted,
            summary: input.summary,
            plan: Some(task.plan.clone()),
            task: Some(task.id.clone()),
            claim: None,
            artifact: None,
            review: None,
            metadata: Value::Null,
        });
        Ok(task)
    }

    pub fn acquire_claim(
        &self,
        meta: EventMeta,
        session_id: SessionId,
        input: ClaimAcquireInput,
    ) -> Result<(
        Option<ClaimId>,
        Vec<crate::types::CoordinationConflict>,
        Option<WorkClaim>,
    )> {
        let mut state = self
            .state
            .write()
            .expect("coordination store lock poisoned");
        expire_claims_locked(&mut state, meta.ts);
        let anchors = dedupe_anchors(input.anchors);
        let policy = plan_policy_for_task(&state, input.task_id.as_ref())?;
        if policy
            .map(|policy| policy.stale_after_graph_change)
            .unwrap_or(false)
            && input.base_revision.graph_version < input.current_revision.graph_version
        {
            return Err(anyhow!(
                "claim acquisition cannot use stale base revision {} when current revision is {}",
                input.base_revision.graph_version,
                input.current_revision.graph_version
            ));
        }
        let mode = input
            .mode
            .or_else(|| policy.map(|policy| policy.default_claim_mode))
            .unwrap_or(ClaimMode::Advisory);
        let mut conflicts = simulate_conflicts(
            state.claims.values(),
            &anchors,
            input.capability,
            mode,
            input.task_id.as_ref(),
            input.base_revision.clone(),
            &session_id,
        );
        conflicts.extend(editor_capacity_conflicts(
            &state,
            &anchors,
            input.capability,
            input.task_id.as_ref(),
            &session_id,
            policy,
            meta.ts,
        ));
        let conflicts = dedupe_conflicts(conflicts);
        let has_blocking = conflicts
            .iter()
            .any(|conflict| conflict.severity == ConflictSeverity::Block);
        if has_blocking {
            state.events.push(CoordinationEvent {
                meta,
                kind: CoordinationEventKind::ClaimContended,
                summary: "claim blocked by active contention".to_string(),
                plan: None,
                task: input.task_id.clone(),
                claim: None,
                artifact: None,
                review: None,
                metadata: Value::Null,
            });
            return Ok((None, conflicts, None));
        }
        state.next_claim += 1;
        let id = ClaimId::new(format!("claim:{}", state.next_claim));
        let claim = WorkClaim {
            id: id.clone(),
            holder: session_id,
            agent: input.agent,
            task: input.task_id,
            anchors,
            capability: input.capability,
            mode,
            since: meta.ts,
            expires_at: meta.ts.saturating_add(input.ttl_seconds.unwrap_or(900)),
            status: if conflicts.is_empty() {
                prism_ir::ClaimStatus::Active
            } else {
                prism_ir::ClaimStatus::Contended
            },
            base_revision: input.base_revision,
        };
        state.claims.insert(id.clone(), claim.clone());
        state.events.push(CoordinationEvent {
            meta: meta.clone(),
            kind: CoordinationEventKind::ClaimAcquired,
            summary: "claim acquired".to_string(),
            plan: None,
            task: claim.task.clone(),
            claim: Some(id.clone()),
            artifact: None,
            review: None,
            metadata: Value::Null,
        });
        if !conflicts.is_empty() {
            state.events.push(CoordinationEvent {
                meta: EventMeta {
                    id: EventId::new(format!("{}:contended", id.0)),
                    ts: meta.ts,
                    actor: meta.actor,
                    correlation: meta.correlation.clone(),
                    causation: Some(meta.id.clone()),
                },
                kind: CoordinationEventKind::ClaimContended,
                summary: "claim acquired with contention".to_string(),
                plan: None,
                task: claim.task.clone(),
                claim: Some(id.clone()),
                artifact: None,
                review: None,
                metadata: Value::Null,
            });
        }
        Ok((Some(id), conflicts, Some(claim)))
    }

    pub fn renew_claim(
        &self,
        meta: EventMeta,
        claim_id: &ClaimId,
        ttl_seconds: Option<u64>,
    ) -> Result<WorkClaim> {
        let mut state = self
            .state
            .write()
            .expect("coordination store lock poisoned");
        expire_claims_locked(&mut state, meta.ts);
        let claim = state
            .claims
            .get_mut(claim_id)
            .ok_or_else(|| anyhow!("unknown claim `{}`", claim_id.0))?;
        if claim.status == prism_ir::ClaimStatus::Expired {
            return Err(anyhow!("claim `{}` has expired", claim_id.0));
        }
        claim.expires_at = meta.ts.saturating_add(ttl_seconds.unwrap_or(900));
        claim.status = prism_ir::ClaimStatus::Active;
        let claim = claim.clone();
        state.events.push(CoordinationEvent {
            meta,
            kind: CoordinationEventKind::ClaimRenewed,
            summary: "claim renewed".to_string(),
            plan: None,
            task: claim.task.clone(),
            claim: Some(claim.id.clone()),
            artifact: None,
            review: None,
            metadata: Value::Null,
        });
        Ok(claim)
    }

    pub fn release_claim(&self, meta: EventMeta, claim_id: &ClaimId) -> Result<WorkClaim> {
        let mut state = self
            .state
            .write()
            .expect("coordination store lock poisoned");
        expire_claims_locked(&mut state, meta.ts);
        let claim = state
            .claims
            .get_mut(claim_id)
            .ok_or_else(|| anyhow!("unknown claim `{}`", claim_id.0))?;
        if claim.status == prism_ir::ClaimStatus::Expired {
            return Err(anyhow!("claim `{}` has expired", claim_id.0));
        }
        claim.status = prism_ir::ClaimStatus::Released;
        let claim = claim.clone();
        state.events.push(CoordinationEvent {
            meta,
            kind: CoordinationEventKind::ClaimReleased,
            summary: "claim released".to_string(),
            plan: None,
            task: claim.task.clone(),
            claim: Some(claim.id.clone()),
            artifact: None,
            review: None,
            metadata: Value::Null,
        });
        Ok(claim)
    }

    pub fn propose_artifact(
        &self,
        meta: EventMeta,
        input: ArtifactProposeInput,
    ) -> Result<(prism_ir::ArtifactId, Artifact)> {
        let mut state = self
            .state
            .write()
            .expect("coordination store lock poisoned");
        let Some(task) = state.tasks.get(&input.task_id).cloned() else {
            return Err(anyhow!("unknown coordination task `{}`", input.task_id.0));
        };
        let plan = state.plans.get(&task.plan).cloned();
        if plan
            .as_ref()
            .map(|plan| plan.policy.stale_after_graph_change)
            .unwrap_or(false)
            && (input.base_revision.graph_version < input.current_revision.graph_version
                || task.base_revision.graph_version < input.current_revision.graph_version)
        {
            return Err(anyhow!(
                "artifact proposal for task `{}` is stale against graph version {}",
                input.task_id.0,
                input.current_revision.graph_version
            ));
        }
        state.next_artifact += 1;
        let id = prism_ir::ArtifactId::new(format!("artifact:{}", state.next_artifact));
        let artifact = Artifact {
            id: id.clone(),
            task: input.task_id.clone(),
            anchors: dedupe_anchors(input.anchors),
            base_revision: input.base_revision,
            diff_ref: input.diff_ref,
            status: ArtifactStatus::Proposed,
            evidence: dedupe_event_ids(input.evidence),
            reviews: Vec::new(),
            required_validations: dedupe_strings(input.required_validations),
            validated_checks: dedupe_strings(input.validated_checks),
            risk_score: input.risk_score,
        };
        state.artifacts.insert(id.clone(), artifact.clone());
        let plan_id = state
            .tasks
            .get(&input.task_id)
            .map(|task| task.plan.clone());
        state.events.push(CoordinationEvent {
            meta,
            kind: CoordinationEventKind::ArtifactProposed,
            summary: "artifact proposed".to_string(),
            plan: plan_id,
            task: Some(input.task_id),
            claim: None,
            artifact: Some(id.clone()),
            review: None,
            metadata: Value::Null,
        });
        Ok((id, artifact))
    }

    pub fn supersede_artifact(
        &self,
        meta: EventMeta,
        input: ArtifactSupersedeInput,
    ) -> Result<Artifact> {
        let mut state = self
            .state
            .write()
            .expect("coordination store lock poisoned");
        let artifact = state
            .artifacts
            .get_mut(&input.artifact_id)
            .ok_or_else(|| anyhow!("unknown artifact `{}`", input.artifact_id.0))?;
        artifact.status = ArtifactStatus::Superseded;
        let artifact = artifact.clone();
        let plan_id = state
            .tasks
            .get(&artifact.task)
            .map(|task| task.plan.clone());
        state.events.push(CoordinationEvent {
            meta,
            kind: CoordinationEventKind::ArtifactSuperseded,
            summary: "artifact superseded".to_string(),
            plan: plan_id,
            task: Some(artifact.task.clone()),
            claim: None,
            artifact: Some(artifact.id.clone()),
            review: None,
            metadata: Value::Null,
        });
        Ok(artifact)
    }

    pub fn review_artifact(
        &self,
        meta: EventMeta,
        input: ArtifactReviewInput,
        current_revision: WorkspaceRevision,
    ) -> Result<(ReviewId, ArtifactReview, Artifact)> {
        let mut state = self
            .state
            .write()
            .expect("coordination store lock poisoned");
        state.next_review += 1;
        let review_id = ReviewId::new(format!("review:{}", state.next_review));
        let (plan, mut artifact): (Option<Plan>, Artifact) = {
            let artifact = state
                .artifacts
                .get(&input.artifact_id)
                .ok_or_else(|| anyhow!("unknown artifact `{}`", input.artifact_id.0))?
                .clone();
            let plan = state
                .tasks
                .get(&artifact.task)
                .and_then(|task| state.plans.get(&task.plan))
                .cloned();
            (plan, artifact)
        };
        if matches!(input.verdict, ReviewVerdict::Approved)
            && plan
                .as_ref()
                .map(|plan| plan.policy.stale_after_graph_change)
                .unwrap_or(false)
            && artifact.base_revision.graph_version < current_revision.graph_version
        {
            return Err(anyhow!(
                "artifact `{}` is stale against graph version {}",
                artifact.id.0,
                current_revision.graph_version
            ));
        }
        let review = ArtifactReview {
            id: review_id.clone(),
            artifact: artifact.id.clone(),
            verdict: input.verdict,
            summary: input.summary.clone(),
            meta: meta.clone(),
        };
        let artifact_mut = state
            .artifacts
            .get_mut(&input.artifact_id)
            .ok_or_else(|| anyhow!("unknown artifact `{}`", input.artifact_id.0))?;
        if !input.required_validations.is_empty() {
            artifact_mut.required_validations = dedupe_strings(input.required_validations.clone());
        }
        if !input.validated_checks.is_empty() {
            let mut checks = artifact_mut.validated_checks.clone();
            checks.extend(input.validated_checks.clone());
            artifact_mut.validated_checks = dedupe_strings(checks);
        }
        if input.risk_score.is_some() {
            artifact_mut.risk_score = input.risk_score;
        }
        if matches!(input.verdict, ReviewVerdict::Approved) {
            let missing = missing_validations_for_artifact(artifact_mut);
            if !missing.is_empty() {
                return Err(anyhow!(
                    "artifact `{}` is missing required validations: {}",
                    artifact_mut.id.0,
                    missing.join(", ")
                ));
            }
        }
        artifact_mut.reviews.push(review_id.clone());
        artifact_mut.status = match input.verdict {
            ReviewVerdict::Approved => ArtifactStatus::Approved,
            ReviewVerdict::ChangesRequested => ArtifactStatus::InReview,
            ReviewVerdict::Rejected => ArtifactStatus::Rejected,
        };
        artifact = artifact_mut.clone();
        state.reviews.insert(review_id.clone(), review.clone());
        let plan_id = state
            .tasks
            .get(&artifact.task)
            .map(|task| task.plan.clone());
        state.events.push(CoordinationEvent {
            meta,
            kind: CoordinationEventKind::ArtifactReviewed,
            summary: input.summary,
            plan: plan_id,
            task: Some(artifact.task.clone()),
            claim: None,
            artifact: Some(artifact.id.clone()),
            review: Some(review_id.clone()),
            metadata: Value::Null,
        });
        Ok((review_id, review, artifact))
    }
}
