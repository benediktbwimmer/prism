use std::collections::{HashMap, HashSet};
use std::sync::RwLock;

use anyhow::{anyhow, Result};
use prism_ir::{
    AgentId, AnchorRef, ArtifactId, ArtifactStatus, Capability, ClaimId, ClaimMode, ClaimStatus,
    ConflictSeverity, CoordinationEventKind, CoordinationTaskId, CoordinationTaskStatus, EventId,
    EventMeta, PlanId, PlanStatus, ReviewId, ReviewVerdict, SessionId, Timestamp,
    WorkspaceRevision,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoordinationPolicy {
    pub default_claim_mode: ClaimMode,
    pub max_parallel_editors_per_anchor: u16,
    pub require_review_for_completion: bool,
    #[serde(default)]
    pub require_validation_for_completion: bool,
    pub stale_after_graph_change: bool,
    #[serde(default)]
    pub review_required_above_risk_score: Option<f32>,
}

impl Default for CoordinationPolicy {
    fn default() -> Self {
        Self {
            default_claim_mode: ClaimMode::Advisory,
            max_parallel_editors_per_anchor: 2,
            require_review_for_completion: false,
            require_validation_for_completion: false,
            stale_after_graph_change: true,
            review_required_above_risk_score: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AcceptanceCriterion {
    pub label: String,
    pub anchors: Vec<AnchorRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    pub id: PlanId,
    pub goal: String,
    pub status: PlanStatus,
    pub policy: CoordinationPolicy,
    pub root_tasks: Vec<CoordinationTaskId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoordinationTask {
    pub id: CoordinationTaskId,
    pub plan: PlanId,
    pub title: String,
    pub status: CoordinationTaskStatus,
    pub assignee: Option<AgentId>,
    pub session: Option<SessionId>,
    pub anchors: Vec<AnchorRef>,
    pub depends_on: Vec<CoordinationTaskId>,
    pub acceptance: Vec<AcceptanceCriterion>,
    pub base_revision: WorkspaceRevision,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkClaim {
    pub id: ClaimId,
    pub holder: SessionId,
    pub agent: Option<AgentId>,
    pub task: Option<CoordinationTaskId>,
    pub anchors: Vec<AnchorRef>,
    pub capability: Capability,
    pub mode: ClaimMode,
    pub since: Timestamp,
    pub expires_at: Timestamp,
    pub status: ClaimStatus,
    pub base_revision: WorkspaceRevision,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoordinationConflict {
    pub severity: ConflictSeverity,
    pub anchors: Vec<AnchorRef>,
    pub summary: String,
    pub blocking_claims: Vec<ClaimId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Artifact {
    pub id: ArtifactId,
    pub task: CoordinationTaskId,
    pub anchors: Vec<AnchorRef>,
    pub base_revision: WorkspaceRevision,
    pub diff_ref: Option<String>,
    pub status: ArtifactStatus,
    pub evidence: Vec<EventId>,
    pub reviews: Vec<ReviewId>,
    #[serde(default)]
    pub required_validations: Vec<String>,
    #[serde(default)]
    pub validated_checks: Vec<String>,
    #[serde(default)]
    pub risk_score: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactReview {
    pub id: ReviewId,
    pub artifact: ArtifactId,
    pub verdict: ReviewVerdict,
    pub summary: String,
    pub meta: EventMeta,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoordinationEvent {
    pub meta: EventMeta,
    pub kind: CoordinationEventKind,
    pub summary: String,
    pub plan: Option<PlanId>,
    pub task: Option<CoordinationTaskId>,
    pub claim: Option<ClaimId>,
    pub artifact: Option<ArtifactId>,
    pub review: Option<ReviewId>,
    pub metadata: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskBlocker {
    pub kind: BlockerKind,
    pub summary: String,
    pub related_task_id: Option<CoordinationTaskId>,
    pub related_artifact_id: Option<ArtifactId>,
    #[serde(default)]
    pub risk_score: Option<f32>,
    #[serde(default)]
    pub validation_checks: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum BlockerKind {
    Dependency,
    ClaimConflict,
    ReviewRequired,
    RiskReviewRequired,
    ValidationRequired,
    StaleRevision,
    ArtifactStale,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CoordinationSnapshot {
    pub plans: Vec<Plan>,
    pub tasks: Vec<CoordinationTask>,
    pub claims: Vec<WorkClaim>,
    pub artifacts: Vec<Artifact>,
    pub reviews: Vec<ArtifactReview>,
    pub events: Vec<CoordinationEvent>,
    pub next_plan: u64,
    pub next_task: u64,
    pub next_claim: u64,
    pub next_artifact: u64,
    pub next_review: u64,
}

#[derive(Debug, Clone)]
pub struct PlanCreateInput {
    pub goal: String,
    pub policy: Option<CoordinationPolicy>,
}

#[derive(Debug, Clone)]
pub struct TaskCreateInput {
    pub plan_id: PlanId,
    pub title: String,
    pub status: Option<CoordinationTaskStatus>,
    pub assignee: Option<AgentId>,
    pub session: Option<SessionId>,
    pub anchors: Vec<AnchorRef>,
    pub depends_on: Vec<CoordinationTaskId>,
    pub acceptance: Vec<AcceptanceCriterion>,
    pub base_revision: WorkspaceRevision,
}

#[derive(Debug, Clone)]
pub struct TaskUpdateInput {
    pub task_id: CoordinationTaskId,
    pub status: Option<CoordinationTaskStatus>,
    pub assignee: Option<Option<AgentId>>,
    pub session: Option<Option<SessionId>>,
    pub title: Option<String>,
    pub anchors: Option<Vec<AnchorRef>>,
    pub base_revision: Option<WorkspaceRevision>,
    pub completion_context: Option<TaskCompletionContext>,
}

#[derive(Debug, Clone, Default)]
pub struct TaskCompletionContext {
    pub risk_score: Option<f32>,
    pub required_validations: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct HandoffInput {
    pub task_id: CoordinationTaskId,
    pub to_agent: Option<AgentId>,
    pub summary: String,
}

#[derive(Debug, Clone)]
pub struct ClaimAcquireInput {
    pub task_id: Option<CoordinationTaskId>,
    pub anchors: Vec<AnchorRef>,
    pub capability: Capability,
    pub mode: Option<ClaimMode>,
    pub ttl_seconds: Option<u64>,
    pub base_revision: WorkspaceRevision,
    pub agent: Option<AgentId>,
}

#[derive(Debug, Clone)]
pub struct ArtifactProposeInput {
    pub task_id: CoordinationTaskId,
    pub anchors: Vec<AnchorRef>,
    pub diff_ref: Option<String>,
    pub evidence: Vec<EventId>,
    pub base_revision: WorkspaceRevision,
    pub required_validations: Vec<String>,
    pub validated_checks: Vec<String>,
    pub risk_score: Option<f32>,
}

#[derive(Debug, Clone)]
pub struct ArtifactSupersedeInput {
    pub artifact_id: ArtifactId,
}

#[derive(Debug, Clone)]
pub struct ArtifactReviewInput {
    pub artifact_id: ArtifactId,
    pub verdict: ReviewVerdict,
    pub summary: String,
    pub required_validations: Vec<String>,
    pub validated_checks: Vec<String>,
    pub risk_score: Option<f32>,
}

#[derive(Default)]
pub struct CoordinationStore {
    state: RwLock<CoordinationState>,
}

#[derive(Default)]
struct CoordinationState {
    plans: HashMap<PlanId, Plan>,
    tasks: HashMap<CoordinationTaskId, CoordinationTask>,
    claims: HashMap<ClaimId, WorkClaim>,
    artifacts: HashMap<ArtifactId, Artifact>,
    reviews: HashMap<ReviewId, ArtifactReview>,
    events: Vec<CoordinationEvent>,
    next_plan: u64,
    next_task: u64,
    next_claim: u64,
    next_artifact: u64,
    next_review: u64,
}

impl CoordinationStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_snapshot(snapshot: CoordinationSnapshot) -> Self {
        Self {
            state: RwLock::new(CoordinationState {
                plans: snapshot
                    .plans
                    .into_iter()
                    .map(|plan| (plan.id.clone(), plan))
                    .collect(),
                tasks: snapshot
                    .tasks
                    .into_iter()
                    .map(|task| (task.id.clone(), task))
                    .collect(),
                claims: snapshot
                    .claims
                    .into_iter()
                    .map(|claim| (claim.id.clone(), claim))
                    .collect(),
                artifacts: snapshot
                    .artifacts
                    .into_iter()
                    .map(|artifact| (artifact.id.clone(), artifact))
                    .collect(),
                reviews: snapshot
                    .reviews
                    .into_iter()
                    .map(|review| (review.id.clone(), review))
                    .collect(),
                events: snapshot.events,
                next_plan: snapshot.next_plan,
                next_task: snapshot.next_task,
                next_claim: snapshot.next_claim,
                next_artifact: snapshot.next_artifact,
                next_review: snapshot.next_review,
            }),
        }
    }

    pub fn snapshot(&self) -> CoordinationSnapshot {
        let state = self.state.read().expect("coordination store lock poisoned");
        CoordinationSnapshot {
            plans: sorted_values(&state.plans, |plan| plan.id.0.to_string()),
            tasks: sorted_values(&state.tasks, |task| task.id.0.to_string()),
            claims: sorted_values(&state.claims, |claim| claim.id.0.to_string()),
            artifacts: sorted_values(&state.artifacts, |artifact| artifact.id.0.to_string()),
            reviews: sorted_values(&state.reviews, |review| review.id.0.to_string()),
            events: state.events.clone(),
            next_plan: state.next_plan,
            next_task: state.next_task,
            next_claim: state.next_claim,
            next_artifact: state.next_artifact,
            next_review: state.next_review,
        }
    }

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
        input: TaskCreateInput,
    ) -> Result<(CoordinationTaskId, CoordinationTask)> {
        let mut state = self
            .state
            .write()
            .expect("coordination store lock poisoned");
        if !state.plans.contains_key(&input.plan_id) {
            return Err(anyhow!("unknown plan `{}`", input.plan_id.0));
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
        let previous;
        let task_snapshot;
        {
            let task = state
                .tasks
                .get_mut(&input.task_id)
                .ok_or_else(|| anyhow!("unknown coordination task `{}`", input.task_id.0))?;
            previous = task.clone();
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
        } else if previous.status != task.status && task.status == CoordinationTaskStatus::Blocked {
            CoordinationEventKind::TaskBlocked
        } else if previous.status == CoordinationTaskStatus::Blocked
            && previous.status != task.status
        {
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

    pub fn handoff(&self, meta: EventMeta, input: HandoffInput) -> Result<CoordinationTask> {
        let mut state = self
            .state
            .write()
            .expect("coordination store lock poisoned");
        let task = state
            .tasks
            .get_mut(&input.task_id)
            .ok_or_else(|| anyhow!("unknown coordination task `{}`", input.task_id.0))?;
        let target_agent = input.to_agent.clone();
        task.assignee = target_agent.clone();
        task.session = None;
        task.status = CoordinationTaskStatus::Ready;
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
        Vec<CoordinationConflict>,
        Option<WorkClaim>,
    )> {
        let mut state = self
            .state
            .write()
            .expect("coordination store lock poisoned");
        expire_claims_locked(&mut state, meta.ts);
        let anchors = dedupe_anchors(input.anchors);
        let policy = plan_policy_for_task(&state, input.task_id.as_ref())?;
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
                ClaimStatus::Active
            } else {
                ClaimStatus::Contended
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
        if claim.status == ClaimStatus::Expired {
            return Err(anyhow!("claim `{}` has expired", claim_id.0));
        }
        claim.expires_at = meta.ts.saturating_add(ttl_seconds.unwrap_or(900));
        claim.status = ClaimStatus::Active;
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
        if claim.status == ClaimStatus::Expired {
            return Err(anyhow!("claim `{}` has expired", claim_id.0));
        }
        claim.status = ClaimStatus::Released;
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
    ) -> Result<(ArtifactId, Artifact)> {
        let mut state = self
            .state
            .write()
            .expect("coordination store lock poisoned");
        if !state.tasks.contains_key(&input.task_id) {
            return Err(anyhow!("unknown coordination task `{}`", input.task_id.0));
        }
        state.next_artifact += 1;
        let id = ArtifactId::new(format!("artifact:{}", state.next_artifact));
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
        let (plan, mut artifact) = {
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

    pub fn plan(&self, id: &PlanId) -> Option<Plan> {
        self.state
            .read()
            .expect("coordination store lock poisoned")
            .plans
            .get(id)
            .cloned()
    }

    pub fn task(&self, id: &CoordinationTaskId) -> Option<CoordinationTask> {
        self.state
            .read()
            .expect("coordination store lock poisoned")
            .tasks
            .get(id)
            .cloned()
    }

    pub fn ready_tasks(
        &self,
        plan_id: &PlanId,
        current_revision: WorkspaceRevision,
        now: Timestamp,
    ) -> Vec<CoordinationTask> {
        let state = self.state.read().expect("coordination store lock poisoned");
        let mut tasks = state
            .tasks
            .values()
            .filter(|task| &task.plan == plan_id)
            .filter(|task| {
                matches!(
                    task.status,
                    CoordinationTaskStatus::Ready | CoordinationTaskStatus::InProgress
                )
            })
            .filter(|task| {
                self.readiness_blockers_locked(&state, task, current_revision.clone(), now)
                    .is_empty()
            })
            .cloned()
            .collect::<Vec<_>>();
        tasks.sort_by(|left, right| left.id.0.cmp(&right.id.0));
        tasks
    }

    pub fn claims_for_anchor(&self, anchors: &[AnchorRef], now: Timestamp) -> Vec<WorkClaim> {
        let state = self.state.read().expect("coordination store lock poisoned");
        let mut claims = state
            .claims
            .values()
            .filter(|claim| claim_is_active(claim, now))
            .filter(|claim| anchors_overlap(&claim.anchors, anchors))
            .cloned()
            .collect::<Vec<_>>();
        claims.sort_by(|left, right| left.id.0.cmp(&right.id.0));
        claims
    }

    pub fn conflicts_for_anchor(
        &self,
        anchors: &[AnchorRef],
        now: Timestamp,
    ) -> Vec<CoordinationConflict> {
        let state = self.state.read().expect("coordination store lock poisoned");
        let relevant = state
            .claims
            .values()
            .filter(|claim| claim_is_active(claim, now))
            .filter(|claim| anchors_overlap(&claim.anchors, anchors))
            .cloned()
            .collect::<Vec<_>>();
        let mut conflicts = Vec::new();
        for (index, claim) in relevant.iter().enumerate() {
            for other in relevant.iter().skip(index + 1) {
                if let Some(conflict) = conflict_between(claim, other) {
                    conflicts.push(conflict);
                }
            }
        }
        dedupe_conflicts(conflicts)
    }

    pub fn simulate_claim(
        &self,
        session_id: &SessionId,
        anchors: &[AnchorRef],
        capability: Capability,
        mode: Option<ClaimMode>,
        task_id: Option<&CoordinationTaskId>,
        revision: WorkspaceRevision,
        now: Timestamp,
    ) -> Vec<CoordinationConflict> {
        let state = self.state.read().expect("coordination store lock poisoned");
        let policy = plan_policy_for_task(&state, task_id).ok().flatten();
        let mode = mode
            .or_else(|| policy.map(|policy| policy.default_claim_mode))
            .unwrap_or(ClaimMode::Advisory);
        let mut conflicts = simulate_conflicts(
            state
                .claims
                .values()
                .filter(|claim| claim_is_active(claim, now)),
            anchors,
            capability,
            mode,
            task_id,
            revision,
            session_id,
        );
        conflicts.extend(editor_capacity_conflicts(
            &state, anchors, capability, task_id, session_id, policy, now,
        ));
        dedupe_conflicts(conflicts)
    }

    pub fn blockers(
        &self,
        task_id: &CoordinationTaskId,
        current_revision: WorkspaceRevision,
        now: Timestamp,
    ) -> Vec<TaskBlocker> {
        let state = self.state.read().expect("coordination store lock poisoned");
        let Some(task) = state.tasks.get(task_id) else {
            return Vec::new();
        };
        self.completion_blockers_locked(&state, task, current_revision, now)
    }

    pub fn pending_reviews(&self, plan_id: Option<&PlanId>) -> Vec<Artifact> {
        let state = self.state.read().expect("coordination store lock poisoned");
        let mut artifacts = state
            .artifacts
            .values()
            .filter(|artifact| {
                matches!(
                    artifact.status,
                    ArtifactStatus::Proposed | ArtifactStatus::InReview
                )
            })
            .filter(|artifact| {
                plan_id.map_or(true, |plan_id| {
                    state
                        .tasks
                        .get(&artifact.task)
                        .map(|task| &task.plan == plan_id)
                        .unwrap_or(false)
                })
            })
            .cloned()
            .collect::<Vec<_>>();
        artifacts.sort_by(|left, right| left.id.0.cmp(&right.id.0));
        artifacts
    }

    pub fn artifacts(&self, task_id: &CoordinationTaskId) -> Vec<Artifact> {
        let state = self.state.read().expect("coordination store lock poisoned");
        let mut artifacts = state
            .artifacts
            .values()
            .filter(|artifact| &artifact.task == task_id)
            .cloned()
            .collect::<Vec<_>>();
        artifacts.sort_by(|left, right| left.id.0.cmp(&right.id.0));
        artifacts
    }

    pub fn events(&self) -> Vec<CoordinationEvent> {
        self.state
            .read()
            .expect("coordination store lock poisoned")
            .events
            .clone()
    }

    fn readiness_blockers_locked(
        &self,
        state: &CoordinationState,
        task: &CoordinationTask,
        current_revision: WorkspaceRevision,
        now: Timestamp,
    ) -> Vec<TaskBlocker> {
        let mut blockers =
            self.dependency_and_revision_blockers_locked(state, task, current_revision);
        blockers.extend(self.claim_blockers_locked(state, task, now));
        blockers
    }

    fn completion_blockers_locked(
        &self,
        state: &CoordinationState,
        task: &CoordinationTask,
        current_revision: WorkspaceRevision,
        now: Timestamp,
    ) -> Vec<TaskBlocker> {
        let mut blockers =
            self.dependency_and_revision_blockers_locked(state, task, current_revision);
        blockers.extend(self.claim_blockers_locked(state, task, now));
        blockers.extend(self.review_blockers_locked(state, task));
        blockers
    }

    fn completion_policy_blockers_locked(
        &self,
        state: &CoordinationState,
        task: &CoordinationTask,
        current_revision: WorkspaceRevision,
        context: Option<&TaskCompletionContext>,
    ) -> Vec<TaskBlocker> {
        let Some(plan) = state.plans.get(&task.plan) else {
            return Vec::new();
        };
        let approved_artifacts = state
            .artifacts
            .values()
            .filter(|artifact| artifact.task == task.id)
            .filter(|artifact| {
                matches!(
                    artifact.status,
                    ArtifactStatus::Approved | ArtifactStatus::Merged
                )
            })
            .collect::<Vec<_>>();
        let mut blockers = Vec::new();

        if plan.policy.stale_after_graph_change {
            let stale = approved_artifacts
                .iter()
                .find(|artifact| {
                    artifact.base_revision.graph_version < current_revision.graph_version
                })
                .map(|artifact| artifact.id.clone());
            if let Some(artifact_id) = stale {
                blockers.push(TaskBlocker {
                    kind: BlockerKind::ArtifactStale,
                    summary: format!(
                        "approved artifact `{}` is stale against graph version {}",
                        artifact_id.0, current_revision.graph_version
                    ),
                    related_task_id: Some(task.id.clone()),
                    related_artifact_id: Some(artifact_id),
                    risk_score: context.and_then(|ctx| ctx.risk_score),
                    validation_checks: Vec::new(),
                });
            }
        }

        if let Some(context) = context {
            if let Some(threshold) = plan.policy.review_required_above_risk_score {
                if context.risk_score.unwrap_or_default() >= threshold
                    && approved_artifacts.is_empty()
                {
                    blockers.push(TaskBlocker {
                        kind: BlockerKind::RiskReviewRequired,
                        summary: format!(
                            "task risk score {:.2} requires review before completion",
                            context.risk_score.unwrap_or_default()
                        ),
                        related_task_id: Some(task.id.clone()),
                        related_artifact_id: None,
                        risk_score: context.risk_score,
                        validation_checks: Vec::new(),
                    });
                }
            }

            if plan.policy.require_validation_for_completion
                && !context.required_validations.is_empty()
            {
                let validated = approved_artifacts
                    .iter()
                    .flat_map(|artifact| artifact.validated_checks.iter().cloned())
                    .collect::<Vec<_>>();
                let validated = dedupe_strings(validated);
                let missing = context
                    .required_validations
                    .iter()
                    .filter(|check| !validated.iter().any(|value| value == *check))
                    .cloned()
                    .collect::<Vec<_>>();
                if !missing.is_empty() {
                    blockers.push(TaskBlocker {
                        kind: BlockerKind::ValidationRequired,
                        summary: format!(
                            "task is missing required validations: {}",
                            missing.join(", ")
                        ),
                        related_task_id: Some(task.id.clone()),
                        related_artifact_id: approved_artifacts
                            .first()
                            .map(|artifact| artifact.id.clone()),
                        risk_score: context.risk_score,
                        validation_checks: missing,
                    });
                }
            }
        }

        blockers
    }

    fn dependency_and_revision_blockers_locked(
        &self,
        state: &CoordinationState,
        task: &CoordinationTask,
        current_revision: WorkspaceRevision,
    ) -> Vec<TaskBlocker> {
        let mut blockers = Vec::new();
        for dep in &task.depends_on {
            match state.tasks.get(dep) {
                Some(dependency) if dependency.status == CoordinationTaskStatus::Completed => {}
                Some(dependency) => blockers.push(TaskBlocker {
                    kind: BlockerKind::Dependency,
                    summary: format!(
                        "dependency `{}` is {:?}",
                        dependency.id.0, dependency.status
                    ),
                    related_task_id: Some(dependency.id.clone()),
                    related_artifact_id: None,
                    risk_score: None,
                    validation_checks: Vec::new(),
                }),
                None => blockers.push(TaskBlocker {
                    kind: BlockerKind::Dependency,
                    summary: format!("dependency `{}` is missing", dep.0),
                    related_task_id: Some(dep.clone()),
                    related_artifact_id: None,
                    risk_score: None,
                    validation_checks: Vec::new(),
                }),
            }
        }

        if let Some(plan) = state.plans.get(&task.plan) {
            if plan.policy.stale_after_graph_change
                && task.base_revision.graph_version < current_revision.graph_version
            {
                blockers.push(TaskBlocker {
                    kind: BlockerKind::StaleRevision,
                    summary: format!(
                        "task is based on graph version {} but current revision is {}",
                        task.base_revision.graph_version, current_revision.graph_version
                    ),
                    related_task_id: Some(task.id.clone()),
                    related_artifact_id: None,
                    risk_score: None,
                    validation_checks: Vec::new(),
                });
            }
        }
        blockers
    }

    fn review_blockers_locked(
        &self,
        state: &CoordinationState,
        task: &CoordinationTask,
    ) -> Vec<TaskBlocker> {
        let Some(plan) = state.plans.get(&task.plan) else {
            return Vec::new();
        };
        if !plan.policy.require_review_for_completion {
            return Vec::new();
        }
        let has_approved = state.artifacts.values().any(|artifact| {
            artifact.task == task.id
                && matches!(
                    artifact.status,
                    ArtifactStatus::Approved | ArtifactStatus::Merged
                )
        });
        if has_approved {
            return Vec::new();
        }
        let pending_artifact = state
            .artifacts
            .values()
            .find(|artifact| artifact.task == task.id)
            .map(|artifact| artifact.id.clone());
        vec![TaskBlocker {
            kind: BlockerKind::ReviewRequired,
            summary: "task requires an approved artifact review".to_string(),
            related_task_id: Some(task.id.clone()),
            related_artifact_id: pending_artifact,
            risk_score: None,
            validation_checks: Vec::new(),
        }]
    }

    fn claim_blockers_locked(
        &self,
        state: &CoordinationState,
        task: &CoordinationTask,
        now: Timestamp,
    ) -> Vec<TaskBlocker> {
        let claim_conflicts = dedupe_conflicts(simulate_conflicts(
            state
                .claims
                .values()
                .filter(|claim| claim_is_active(claim, now)),
            &task.anchors,
            Capability::Edit,
            state
                .plans
                .get(&task.plan)
                .map(|plan| plan.policy.default_claim_mode)
                .unwrap_or(ClaimMode::SoftExclusive),
            Some(&task.id),
            task.base_revision.clone(),
            task.session
                .as_ref()
                .unwrap_or(&SessionId::new("session:none")),
        ));
        let mut blockers = Vec::new();
        for conflict in claim_conflicts {
            if conflict.severity != ConflictSeverity::Block {
                continue;
            }
            blockers.push(TaskBlocker {
                kind: BlockerKind::ClaimConflict,
                summary: conflict.summary,
                related_task_id: Some(task.id.clone()),
                related_artifact_id: None,
                risk_score: None,
                validation_checks: Vec::new(),
            });
        }
        blockers
    }
}

fn plan_policy_for_task<'a>(
    state: &'a CoordinationState,
    task_id: Option<&CoordinationTaskId>,
) -> Result<Option<&'a CoordinationPolicy>> {
    let Some(task_id) = task_id else {
        return Ok(None);
    };
    let task = state
        .tasks
        .get(task_id)
        .ok_or_else(|| anyhow!("unknown coordination task `{}`", task_id.0))?;
    Ok(state.plans.get(&task.plan).map(|plan| &plan.policy))
}

fn expire_claims_locked(state: &mut CoordinationState, now: Timestamp) {
    for claim in state.claims.values_mut() {
        if matches!(claim.status, ClaimStatus::Active | ClaimStatus::Contended)
            && claim.expires_at < now
        {
            claim.status = ClaimStatus::Expired;
        }
    }
}

fn editor_capacity_conflicts(
    state: &CoordinationState,
    anchors: &[AnchorRef],
    capability: Capability,
    task_id: Option<&CoordinationTaskId>,
    session_id: &SessionId,
    policy: Option<&CoordinationPolicy>,
    now: Timestamp,
) -> Vec<CoordinationConflict> {
    if !matches!(capability, Capability::Edit | Capability::Merge) {
        return Vec::new();
    }
    let requested_limit =
        policy.map(|policy| usize::from(policy.max_parallel_editors_per_anchor.max(1)));
    let candidates = state
        .claims
        .values()
        .filter(|claim| claim_is_active(claim, now))
        .filter(|claim| matches!(claim.capability, Capability::Edit | Capability::Merge))
        .filter(|claim| &claim.holder != session_id)
        .filter(|claim| task_id.map_or(true, |task| claim.task.as_ref() != Some(task)))
        .collect::<Vec<_>>();
    let mut conflicts = Vec::new();
    for anchor in anchors {
        let overlapping = candidates
            .iter()
            .copied()
            .filter(|claim| claim.anchors.contains(anchor))
            .collect::<Vec<_>>();
        let limit = overlapping
            .iter()
            .filter_map(|claim| {
                claim
                    .task
                    .as_ref()
                    .and_then(|claim_task_id| {
                        plan_policy_for_task(state, Some(claim_task_id))
                            .ok()
                            .flatten()
                    })
                    .map(|policy| usize::from(policy.max_parallel_editors_per_anchor.max(1)))
            })
            .chain(requested_limit)
            .min();
        let Some(limit) = limit else {
            continue;
        };
        if overlapping.len() >= limit {
            conflicts.push(CoordinationConflict {
                severity: ConflictSeverity::Block,
                anchors: vec![anchor.clone()],
                summary: format!("anchor is already at the edit concurrency limit ({limit})"),
                blocking_claims: overlapping
                    .into_iter()
                    .map(|claim| claim.id.clone())
                    .collect(),
            });
        }
    }
    conflicts
}

fn dedupe_anchors(mut anchors: Vec<AnchorRef>) -> Vec<AnchorRef> {
    anchors.sort_by_key(anchor_sort_key);
    anchors.dedup();
    anchors
}

fn normalize_acceptance(mut acceptance: Vec<AcceptanceCriterion>) -> Vec<AcceptanceCriterion> {
    for criterion in &mut acceptance {
        criterion.anchors = dedupe_anchors(criterion.anchors.clone());
    }
    acceptance.sort_by(|left, right| left.label.cmp(&right.label));
    acceptance
}

fn dedupe_ids<T>(mut ids: Vec<T>) -> Vec<T>
where
    T: Ord,
{
    ids.sort();
    ids.dedup();
    ids
}

fn dedupe_event_ids(mut ids: Vec<EventId>) -> Vec<EventId> {
    ids.sort_by(|left, right| left.0.cmp(&right.0));
    ids.dedup_by(|left, right| left.0 == right.0);
    ids
}

fn dedupe_strings(mut values: Vec<String>) -> Vec<String> {
    values.sort();
    values.dedup();
    values
}

fn missing_validations_for_artifact(artifact: &Artifact) -> Vec<String> {
    artifact
        .required_validations
        .iter()
        .filter(|check| {
            !artifact
                .validated_checks
                .iter()
                .any(|value| value == *check)
        })
        .cloned()
        .collect()
}

fn claim_is_active(claim: &WorkClaim, now: Timestamp) -> bool {
    matches!(claim.status, ClaimStatus::Active | ClaimStatus::Contended) && claim.expires_at >= now
}

fn anchors_overlap(left: &[AnchorRef], right: &[AnchorRef]) -> bool {
    if left.is_empty() || right.is_empty() {
        return false;
    }
    let right = right.iter().collect::<HashSet<_>>();
    left.iter().any(|anchor| right.contains(anchor))
}

fn simulate_conflicts<'a, I>(
    claims: I,
    anchors: &[AnchorRef],
    capability: Capability,
    mode: ClaimMode,
    task_id: Option<&CoordinationTaskId>,
    revision: WorkspaceRevision,
    session_id: &SessionId,
) -> Vec<CoordinationConflict>
where
    I: IntoIterator<Item = &'a WorkClaim>,
{
    claims
        .into_iter()
        .filter(|claim| anchors_overlap(&claim.anchors, anchors))
        .filter(|claim| &claim.holder != session_id)
        .filter(|claim| task_id.map_or(true, |task| claim.task.as_ref() != Some(task)))
        .filter_map(|claim| proposal_conflict(claim, anchors, capability, mode, revision.clone()))
        .collect()
}

fn proposal_conflict(
    claim: &WorkClaim,
    anchors: &[AnchorRef],
    capability: Capability,
    mode: ClaimMode,
    revision: WorkspaceRevision,
) -> Option<CoordinationConflict> {
    let overlap = overlapping_anchors(&claim.anchors, anchors);
    if overlap.is_empty() {
        return None;
    }
    let severity = conflict_severity(
        claim.capability,
        claim.mode,
        capability,
        mode,
        claim.base_revision.clone(),
        revision,
    );
    Some(CoordinationConflict {
        severity,
        summary: format!(
            "claim `{}` conflicts with {:?}/{:?} on {} anchor(s)",
            claim.id.0,
            claim.capability,
            claim.mode,
            overlap.len()
        ),
        anchors: overlap,
        blocking_claims: vec![claim.id.clone()],
    })
}

fn conflict_between(left: &WorkClaim, right: &WorkClaim) -> Option<CoordinationConflict> {
    let overlap = overlapping_anchors(&left.anchors, &right.anchors);
    if overlap.is_empty() {
        return None;
    }
    Some(CoordinationConflict {
        severity: conflict_severity(
            left.capability,
            left.mode,
            right.capability,
            right.mode,
            left.base_revision.clone(),
            right.base_revision.clone(),
        ),
        summary: format!("claims `{}` and `{}` overlap", left.id.0, right.id.0),
        anchors: overlap,
        blocking_claims: vec![left.id.clone(), right.id.clone()],
    })
}

fn overlapping_anchors(left: &[AnchorRef], right: &[AnchorRef]) -> Vec<AnchorRef> {
    let right = right.iter().collect::<HashSet<_>>();
    let mut overlap = left
        .iter()
        .filter(|anchor| right.contains(anchor))
        .cloned()
        .collect::<Vec<_>>();
    overlap.sort_by_key(anchor_sort_key);
    overlap.dedup();
    overlap
}

fn conflict_severity(
    left_capability: Capability,
    left_mode: ClaimMode,
    right_capability: Capability,
    right_mode: ClaimMode,
    left_revision: WorkspaceRevision,
    right_revision: WorkspaceRevision,
) -> ConflictSeverity {
    let left_write = matches!(left_capability, Capability::Edit | Capability::Merge);
    let right_write = matches!(right_capability, Capability::Edit | Capability::Merge);
    if matches!(left_mode, ClaimMode::HardExclusive)
        || matches!(right_mode, ClaimMode::HardExclusive)
    {
        return ConflictSeverity::Block;
    }
    if left_write && right_write {
        return ConflictSeverity::Warn;
    }
    if left_revision.graph_version != right_revision.graph_version {
        return ConflictSeverity::Warn;
    }
    ConflictSeverity::Info
}

fn dedupe_conflicts(mut conflicts: Vec<CoordinationConflict>) -> Vec<CoordinationConflict> {
    conflicts.sort_by(|left, right| {
        severity_rank(left.severity)
            .cmp(&severity_rank(right.severity))
            .then_with(|| left.summary.cmp(&right.summary))
    });
    conflicts.dedup_by(|left, right| {
        left.severity == right.severity
            && left.summary == right.summary
            && left.blocking_claims == right.blocking_claims
    });
    conflicts
}

fn severity_rank(severity: ConflictSeverity) -> u8 {
    match severity {
        ConflictSeverity::Info => 0,
        ConflictSeverity::Warn => 1,
        ConflictSeverity::Block => 2,
    }
}

fn sorted_values<K, V, F>(values: &HashMap<K, V>, key: F) -> Vec<V>
where
    K: Eq + std::hash::Hash,
    V: Clone,
    F: Fn(&V) -> String,
{
    let mut items = values.values().cloned().collect::<Vec<_>>();
    items.sort_by(|left, right| key(left).cmp(&key(right)));
    items
}

fn anchor_sort_key(anchor: &AnchorRef) -> (u8, String) {
    match anchor {
        AnchorRef::Node(node) => (
            0,
            format!("{}::{}::{:?}", node.crate_name, node.path, node.kind),
        ),
        AnchorRef::Lineage(lineage) => (1, lineage.0.to_string()),
        AnchorRef::File(file_id) => (2, file_id.0.to_string()),
        AnchorRef::Kind(kind) => (3, format!("{kind:?}")),
    }
}

#[cfg(test)]
mod tests {
    use prism_ir::{EventActor, EventMeta};

    use super::*;

    fn meta(id: &str, ts: u64) -> EventMeta {
        EventMeta {
            id: EventId::new(id),
            ts,
            actor: EventActor::Agent,
            correlation: None,
            causation: None,
        }
    }

    #[test]
    fn claim_conflicts_block_hard_exclusive_overlap() {
        let store = CoordinationStore::new();
        let (plan_id, _) = store
            .create_plan(
                meta("event:1", 1),
                PlanCreateInput {
                    goal: "Ship coordination".to_string(),
                    policy: None,
                },
            )
            .unwrap();
        let (task_id, task) = store
            .create_task(
                meta("event:2", 2),
                TaskCreateInput {
                    plan_id,
                    title: "Edit auth".to_string(),
                    status: Some(CoordinationTaskStatus::Ready),
                    assignee: None,
                    session: Some(SessionId::new("session:a")),
                    anchors: vec![AnchorRef::Kind(prism_ir::NodeKind::Function)],
                    depends_on: Vec::new(),
                    acceptance: Vec::new(),
                    base_revision: WorkspaceRevision {
                        graph_version: 1,
                        git_commit: None,
                    },
                },
            )
            .unwrap();

        let first = store
            .acquire_claim(
                meta("event:3", 3),
                SessionId::new("session:a"),
                ClaimAcquireInput {
                    task_id: Some(task_id.clone()),
                    anchors: task.anchors.clone(),
                    capability: Capability::Edit,
                    mode: Some(ClaimMode::HardExclusive),
                    ttl_seconds: Some(60),
                    base_revision: WorkspaceRevision {
                        graph_version: 1,
                        git_commit: None,
                    },
                    agent: None,
                },
            )
            .unwrap();
        assert!(first.0.is_some());

        let second = store
            .acquire_claim(
                meta("event:4", 4),
                SessionId::new("session:b"),
                ClaimAcquireInput {
                    task_id: None,
                    anchors: task.anchors.clone(),
                    capability: Capability::Edit,
                    mode: Some(ClaimMode::HardExclusive),
                    ttl_seconds: Some(60),
                    base_revision: WorkspaceRevision {
                        graph_version: 1,
                        git_commit: None,
                    },
                    agent: None,
                },
            )
            .unwrap();
        assert!(second.0.is_none());
        assert_eq!(second.1[0].severity, ConflictSeverity::Block);
    }

    #[test]
    fn review_policy_gates_completion_but_not_ready_work() {
        let store = CoordinationStore::new();
        let (plan_id, _) = store
            .create_plan(
                meta("event:1", 1),
                PlanCreateInput {
                    goal: "Ship reviewed change".to_string(),
                    policy: Some(CoordinationPolicy {
                        require_review_for_completion: true,
                        ..CoordinationPolicy::default()
                    }),
                },
            )
            .unwrap();
        let (task_id, _) = store
            .create_task(
                meta("event:2", 2),
                TaskCreateInput {
                    plan_id,
                    title: "Edit main".to_string(),
                    status: Some(CoordinationTaskStatus::Ready),
                    assignee: None,
                    session: Some(SessionId::new("session:a")),
                    anchors: vec![AnchorRef::Kind(prism_ir::NodeKind::Function)],
                    depends_on: Vec::new(),
                    acceptance: Vec::new(),
                    base_revision: WorkspaceRevision {
                        graph_version: 1,
                        git_commit: None,
                    },
                },
            )
            .unwrap();

        assert_eq!(
            store
                .ready_tasks(
                    &PlanId::new("plan:1"),
                    WorkspaceRevision {
                        graph_version: 1,
                        git_commit: None,
                    },
                    2,
                )
                .len(),
            1
        );
        assert!(store
            .update_task(
                meta("event:3", 3),
                TaskUpdateInput {
                    task_id: task_id.clone(),
                    status: Some(CoordinationTaskStatus::Completed),
                    assignee: None,
                    session: None,
                    title: None,
                    anchors: None,
                    base_revision: None,
                    completion_context: Some(TaskCompletionContext::default()),
                },
                WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                3,
            )
            .is_err());

        let (artifact_id, _) = store
            .propose_artifact(
                meta("event:4", 4),
                ArtifactProposeInput {
                    task_id: task_id.clone(),
                    anchors: vec![AnchorRef::Kind(prism_ir::NodeKind::Function)],
                    diff_ref: Some("patch:1".to_string()),
                    evidence: Vec::new(),
                    base_revision: WorkspaceRevision {
                        graph_version: 1,
                        git_commit: None,
                    },
                    required_validations: Vec::new(),
                    validated_checks: Vec::new(),
                    risk_score: None,
                },
            )
            .unwrap();
        store
            .review_artifact(
                meta("event:5", 5),
                ArtifactReviewInput {
                    artifact_id,
                    verdict: ReviewVerdict::Approved,
                    summary: "looks good".to_string(),
                    required_validations: Vec::new(),
                    validated_checks: Vec::new(),
                    risk_score: None,
                },
                WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
            )
            .unwrap();
        assert_eq!(
            store
                .update_task(
                    meta("event:6", 6),
                    TaskUpdateInput {
                        task_id,
                        status: Some(CoordinationTaskStatus::Completed),
                        assignee: None,
                        session: None,
                        title: None,
                        anchors: None,
                        base_revision: None,
                        completion_context: Some(TaskCompletionContext::default()),
                    },
                    WorkspaceRevision {
                        graph_version: 1,
                        git_commit: None,
                    },
                    6,
                )
                .unwrap()
                .status,
            CoordinationTaskStatus::Completed
        );
    }

    #[test]
    fn edit_capacity_limit_blocks_extra_claims() {
        let store = CoordinationStore::new();
        let (plan_id, _) = store
            .create_plan(
                meta("event:1", 1),
                PlanCreateInput {
                    goal: "Serialize edits".to_string(),
                    policy: Some(CoordinationPolicy {
                        max_parallel_editors_per_anchor: 1,
                        ..CoordinationPolicy::default()
                    }),
                },
            )
            .unwrap();
        let (task_id, task) = store
            .create_task(
                meta("event:2", 2),
                TaskCreateInput {
                    plan_id,
                    title: "Edit main".to_string(),
                    status: Some(CoordinationTaskStatus::Ready),
                    assignee: None,
                    session: Some(SessionId::new("session:a")),
                    anchors: vec![AnchorRef::Kind(prism_ir::NodeKind::Function)],
                    depends_on: Vec::new(),
                    acceptance: Vec::new(),
                    base_revision: WorkspaceRevision {
                        graph_version: 1,
                        git_commit: None,
                    },
                },
            )
            .unwrap();
        assert!(store
            .acquire_claim(
                meta("event:3", 3),
                SessionId::new("session:a"),
                ClaimAcquireInput {
                    task_id: Some(task_id),
                    anchors: task.anchors.clone(),
                    capability: Capability::Edit,
                    mode: Some(ClaimMode::SoftExclusive),
                    ttl_seconds: Some(60),
                    base_revision: WorkspaceRevision {
                        graph_version: 1,
                        git_commit: None,
                    },
                    agent: None,
                },
            )
            .unwrap()
            .0
            .is_some());

        let blocked = store
            .acquire_claim(
                meta("event:4", 4),
                SessionId::new("session:b"),
                ClaimAcquireInput {
                    task_id: None,
                    anchors: task.anchors.clone(),
                    capability: Capability::Edit,
                    mode: Some(ClaimMode::SoftExclusive),
                    ttl_seconds: Some(60),
                    base_revision: WorkspaceRevision {
                        graph_version: 1,
                        git_commit: None,
                    },
                    agent: None,
                },
            )
            .unwrap();
        assert!(blocked.0.is_none());
        assert!(blocked
            .1
            .iter()
            .any(|conflict| conflict.severity == ConflictSeverity::Block));
    }

    #[test]
    fn approving_stale_artifact_is_rejected() {
        let store = CoordinationStore::new();
        let (plan_id, _) = store
            .create_plan(
                meta("event:1", 1),
                PlanCreateInput {
                    goal: "Catch stale approvals".to_string(),
                    policy: Some(CoordinationPolicy {
                        stale_after_graph_change: true,
                        ..CoordinationPolicy::default()
                    }),
                },
            )
            .unwrap();
        let (task_id, _) = store
            .create_task(
                meta("event:2", 2),
                TaskCreateInput {
                    plan_id,
                    title: "Edit main".to_string(),
                    status: Some(CoordinationTaskStatus::Ready),
                    assignee: None,
                    session: Some(SessionId::new("session:a")),
                    anchors: vec![AnchorRef::Kind(prism_ir::NodeKind::Function)],
                    depends_on: Vec::new(),
                    acceptance: Vec::new(),
                    base_revision: WorkspaceRevision {
                        graph_version: 1,
                        git_commit: None,
                    },
                },
            )
            .unwrap();
        let (artifact_id, _) = store
            .propose_artifact(
                meta("event:3", 3),
                ArtifactProposeInput {
                    task_id,
                    anchors: vec![AnchorRef::Kind(prism_ir::NodeKind::Function)],
                    diff_ref: Some("patch:1".to_string()),
                    evidence: Vec::new(),
                    base_revision: WorkspaceRevision {
                        graph_version: 1,
                        git_commit: None,
                    },
                    required_validations: Vec::new(),
                    validated_checks: Vec::new(),
                    risk_score: None,
                },
            )
            .unwrap();

        assert!(store
            .review_artifact(
                meta("event:4", 4),
                ArtifactReviewInput {
                    artifact_id,
                    verdict: ReviewVerdict::Approved,
                    summary: "approve stale patch".to_string(),
                    required_validations: Vec::new(),
                    validated_checks: Vec::new(),
                    risk_score: None,
                },
                WorkspaceRevision {
                    graph_version: 2,
                    git_commit: None,
                },
            )
            .is_err());
    }

    #[test]
    fn validation_policy_requires_approved_artifact_checks() {
        let store = CoordinationStore::new();
        let (plan_id, _) = store
            .create_plan(
                meta("event:1", 1),
                PlanCreateInput {
                    goal: "Validate risky change".to_string(),
                    policy: Some(CoordinationPolicy {
                        require_validation_for_completion: true,
                        ..CoordinationPolicy::default()
                    }),
                },
            )
            .unwrap();
        let (task_id, _) = store
            .create_task(
                meta("event:2", 2),
                TaskCreateInput {
                    plan_id,
                    title: "Edit main".to_string(),
                    status: Some(CoordinationTaskStatus::Ready),
                    assignee: None,
                    session: Some(SessionId::new("session:a")),
                    anchors: vec![AnchorRef::Kind(prism_ir::NodeKind::Function)],
                    depends_on: Vec::new(),
                    acceptance: Vec::new(),
                    base_revision: WorkspaceRevision {
                        graph_version: 1,
                        git_commit: None,
                    },
                },
            )
            .unwrap();
        let (artifact_id, _) = store
            .propose_artifact(
                meta("event:3", 3),
                ArtifactProposeInput {
                    task_id: task_id.clone(),
                    anchors: vec![AnchorRef::Kind(prism_ir::NodeKind::Function)],
                    diff_ref: Some("patch:1".to_string()),
                    evidence: Vec::new(),
                    base_revision: WorkspaceRevision {
                        graph_version: 1,
                        git_commit: None,
                    },
                    required_validations: vec!["test:main_integration".to_string()],
                    validated_checks: Vec::new(),
                    risk_score: Some(0.4),
                },
            )
            .unwrap();

        assert!(store
            .review_artifact(
                meta("event:4", 4),
                ArtifactReviewInput {
                    artifact_id: artifact_id.clone(),
                    verdict: ReviewVerdict::Approved,
                    summary: "missing validation".to_string(),
                    required_validations: vec!["test:main_integration".to_string()],
                    validated_checks: Vec::new(),
                    risk_score: Some(0.4),
                },
                WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
            )
            .is_err());

        store
            .review_artifact(
                meta("event:5", 5),
                ArtifactReviewInput {
                    artifact_id,
                    verdict: ReviewVerdict::Approved,
                    summary: "validated".to_string(),
                    required_validations: vec!["test:main_integration".to_string()],
                    validated_checks: vec!["test:main_integration".to_string()],
                    risk_score: Some(0.4),
                },
                WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
            )
            .unwrap();

        assert_eq!(
            store
                .update_task(
                    meta("event:6", 6),
                    TaskUpdateInput {
                        task_id,
                        status: Some(CoordinationTaskStatus::Completed),
                        assignee: None,
                        session: None,
                        title: None,
                        anchors: None,
                        base_revision: None,
                        completion_context: Some(TaskCompletionContext {
                            risk_score: Some(0.4),
                            required_validations: vec!["test:main_integration".to_string()],
                        }),
                    },
                    WorkspaceRevision {
                        graph_version: 1,
                        git_commit: None,
                    },
                    6,
                )
                .unwrap()
                .status,
            CoordinationTaskStatus::Completed
        );
    }

    #[test]
    fn risk_threshold_requires_review_before_completion() {
        let store = CoordinationStore::new();
        let (plan_id, _) = store
            .create_plan(
                meta("event:1", 1),
                PlanCreateInput {
                    goal: "Risky edit".to_string(),
                    policy: Some(CoordinationPolicy {
                        review_required_above_risk_score: Some(0.5),
                        ..CoordinationPolicy::default()
                    }),
                },
            )
            .unwrap();
        let (task_id, _) = store
            .create_task(
                meta("event:2", 2),
                TaskCreateInput {
                    plan_id,
                    title: "Edit main".to_string(),
                    status: Some(CoordinationTaskStatus::Ready),
                    assignee: None,
                    session: Some(SessionId::new("session:a")),
                    anchors: vec![AnchorRef::Kind(prism_ir::NodeKind::Function)],
                    depends_on: Vec::new(),
                    acceptance: Vec::new(),
                    base_revision: WorkspaceRevision {
                        graph_version: 1,
                        git_commit: None,
                    },
                },
            )
            .unwrap();

        assert!(store
            .update_task(
                meta("event:3", 3),
                TaskUpdateInput {
                    task_id: task_id.clone(),
                    status: Some(CoordinationTaskStatus::Completed),
                    assignee: None,
                    session: None,
                    title: None,
                    anchors: None,
                    base_revision: None,
                    completion_context: Some(TaskCompletionContext {
                        risk_score: Some(0.8),
                        required_validations: Vec::new(),
                    }),
                },
                WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                3,
            )
            .is_err());
    }
}
