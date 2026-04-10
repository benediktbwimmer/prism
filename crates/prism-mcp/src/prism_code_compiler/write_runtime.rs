use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
use serde_json::{json, Map, Value};

use super::analysis::AnalyzedPrismProgram;
use super::program_ir::{PrismProgramEffectKind, PrismProgramRegionId};
use super::transaction_plan::{StructuredTransactionEffectMetadata, StructuredTransactionPlan};
use crate::PrismDeclareWorkArgs;

pub(crate) type CoordinationCommitExecutor = Arc<dyn Fn(Value) -> Result<Value> + Send + Sync>;

const HANDLE_KIND_KEY: &str = "__prismCoordinationHandleKind";
const HANDLE_ID_KEY: &str = "__prismCoordinationHandleId";
const PLAN_HANDLE_KIND: &str = "plan";
const TASK_HANDLE_KIND: &str = "task";
const CLAIM_HANDLE_KIND: &str = "claim";
const ARTIFACT_HANDLE_KIND: &str = "artifact";
const WORK_HANDLE_KIND: &str = "work";

type StructuredWriteMetadata = StructuredTransactionEffectMetadata;

#[derive(Debug, Clone, serde::Serialize)]
enum StructuredWriteOperationKind {
    Coordination(CoordinationWriteOp),
    Direct(StructuredDirectWriteOp),
}

#[derive(Debug, Clone, serde::Serialize)]
enum CoordinationWriteOp {
    CreatePlan {
        client_plan_id: String,
        title: String,
        goal: String,
        status: Option<String>,
        policy: Option<Value>,
        scheduling: Option<Value>,
    },
    PlanUpdate {
        plan_handle_id: String,
        title: Option<String>,
        goal: Option<String>,
        status: Option<String>,
        policy: Option<Value>,
        scheduling: Option<Value>,
    },
    PlanArchive {
        plan_handle_id: String,
    },
    CreateTask {
        plan_handle_id: String,
        client_task_id: String,
        title: String,
        status: Option<String>,
        depends_on: Vec<String>,
        assignee: Option<Value>,
        anchors: Option<Value>,
        acceptance: Option<Value>,
        artifact_requirements: Option<Value>,
        review_requirements: Option<Value>,
    },
    TaskUpdate {
        task_handle_id: String,
        status: Option<String>,
        title: Option<String>,
        summary: Option<Value>,
        assignee: Option<Value>,
        priority: Option<Value>,
        depends_on: Option<Vec<String>>,
        anchors: Option<Value>,
        acceptance: Option<Value>,
        validation_refs: Option<Value>,
        tags: Option<Value>,
        artifact_requirements: Option<Value>,
        review_requirements: Option<Value>,
    },
    TaskDependsOn {
        task_handle_id: String,
        depends_on_handle_id: String,
        kind: String,
    },
    ClaimAcquire {
        client_claim_id: String,
        task_handle_id: Option<String>,
        anchors: Value,
        capability: Value,
        mode: Option<Value>,
        ttl_seconds: Option<u64>,
        agent: Option<String>,
    },
    ClaimRenew {
        claim_handle_id: String,
        ttl_seconds: Option<u64>,
    },
    ClaimRelease {
        claim_handle_id: String,
    },
    ArtifactPropose {
        client_artifact_id: String,
        task_handle_id: String,
        artifact_requirement_id: Option<String>,
        anchors: Option<Value>,
        diff_ref: Option<String>,
        evidence: Option<Value>,
        required_validations: Option<Value>,
        validated_checks: Option<Value>,
        risk_score: Option<f64>,
    },
    ArtifactSupersede {
        artifact_handle_id: String,
    },
    ArtifactReview {
        artifact_handle_id: String,
        review_requirement_id: Option<String>,
        verdict: Value,
        summary: String,
        required_validations: Option<Value>,
        validated_checks: Option<Value>,
        risk_score: Option<f64>,
    },
    TaskHandoff {
        task_handle_id: String,
        summary: String,
        to_agent: Option<String>,
    },
    TaskAcceptHandoff {
        task_handle_id: String,
        agent: Option<String>,
    },
    TaskResume {
        task_handle_id: String,
        agent: Option<String>,
    },
    TaskReclaim {
        task_handle_id: String,
        agent: Option<String>,
    },
}

#[derive(Debug, Clone, serde::Serialize)]
enum StructuredDirectWriteOp {
    DeclareWork {
        handle_id: String,
        input: Value,
    },
}

#[derive(Debug, Clone)]
enum PlanHandleState {
    Created {
        client_plan_id: String,
        committed_plan_id: Option<String>,
        current: Value,
    },
    Existing {
        plan_id: String,
        current: Value,
    },
}

enum TaskHandleState {
    Created {
        client_task_id: String,
        committed_task_id: Option<String>,
        current: Value,
    },
    Existing {
        task_id: String,
        current: Value,
    },
}

struct DeferredHandleState {
    client_id: Option<String>,
    current: Value,
}

struct PrismCodeWriteState {
    next_plan_handle: usize,
    next_task_handle: usize,
    next_claim_handle: usize,
    next_artifact_handle: usize,
    next_work_handle: usize,
    next_client_plan: usize,
    next_client_task: usize,
    next_client_claim: usize,
    next_client_artifact: usize,
    transaction_plan: StructuredTransactionPlan<StructuredWriteOperationKind>,
    plan_handles: BTreeMap<String, PlanHandleState>,
    task_handles: BTreeMap<String, TaskHandleState>,
    claim_handles: BTreeMap<String, DeferredHandleState>,
    artifact_handles: BTreeMap<String, DeferredHandleState>,
    work_handles: BTreeMap<String, DeferredHandleState>,
}

#[derive(Clone)]
pub(crate) struct PrismCodeWriteRuntimeFactory {
    coordination_executor: CoordinationCommitExecutor,
    dry_run: bool,
}

#[derive(Clone)]
pub(crate) struct PrismCodeWriteRuntime {
    coordination_executor: CoordinationCommitExecutor,
    dry_run: bool,
    analyzed: Arc<AnalyzedPrismProgram>,
    effect_cursors: Arc<Mutex<HashMap<String, usize>>>,
    state: Arc<Mutex<PrismCodeWriteState>>,
}

impl PrismCodeWriteRuntimeFactory {
    pub(crate) fn new(coordination_executor: CoordinationCommitExecutor, dry_run: bool) -> Self {
        Self {
            coordination_executor,
            dry_run,
        }
    }

    pub(crate) fn instantiate(&self, analyzed: AnalyzedPrismProgram) -> PrismCodeWriteRuntime {
        let state = PrismCodeWriteState::new(&analyzed);
        PrismCodeWriteRuntime {
            coordination_executor: Arc::clone(&self.coordination_executor),
            dry_run: self.dry_run,
            analyzed: Arc::new(analyzed),
            effect_cursors: Arc::new(Mutex::new(HashMap::new())),
            state: Arc::new(Mutex::new(state)),
        }
    }
}

impl PrismCodeWriteRuntime {
    pub(crate) fn compiler_write(&self, method_path: &str, payload: Value) -> Result<Value> {
        let payload = Value::Object(expect_object(payload, method_path)?);
        let object = payload
            .as_object()
            .expect("payload object should remain an object");
        match method_path {
            "prism.work.declare" => self.declare_work(required_payload_value(object, "input", method_path)?),
            "prism.claim.acquire" => self.claim_acquire(required_payload_value(object, "input", method_path)?),
            "prism.claim.renew" => self.claim_renew(
                required_payload_value(object, "claim", method_path)?,
                object.get("input").cloned().unwrap_or_else(|| json!({})),
            ),
            "prism.claim.release" => {
                self.claim_release(required_payload_value(object, "claim", method_path)?)
            }
            "prism.artifact.propose" => {
                self.artifact_propose(required_payload_value(object, "input", method_path)?)
            }
            "prism.artifact.supersede" => self
                .artifact_supersede(required_payload_value(object, "artifact", method_path)?),
            "prism.artifact.review" => self.artifact_review(
                required_payload_value(object, "artifact", method_path)?,
                required_payload_value(object, "input", method_path)?,
            ),
            "prism.coordination.createPlan" => {
                self.create_plan(required_payload_value(object, "input", method_path)?)
            }
            "prism.coordination.openPlan" => {
                self.open_plan(required_payload_string(object, "planId", method_path)?)
            }
            "prism.coordination.openTask" => {
                self.open_task(required_payload_string(object, "taskId", method_path)?)
            }
            "plan.update" => self.plan_update(
                required_payload_value(object, "plan", method_path)?,
                object.get("input").cloned().unwrap_or_else(|| json!({})),
            ),
            "plan.archive" => {
                self.plan_archive(required_payload_value(object, "plan", method_path)?)
            }
            "plan.addTask" => self.plan_add_task(
                required_payload_string(object, "planHandleId", method_path)?,
                required_payload_value(object, "input", method_path)?,
            ),
            "task.dependsOn" => self.task_depends_on(
                required_payload_value(object, "task", method_path)?,
                required_payload_value(object, "dependsOn", method_path)?,
                optional_payload_string(object, "kind"),
            ),
            "task.update" => self.task_update(
                required_payload_value(object, "task", method_path)?,
                object.get("input").cloned().unwrap_or_else(|| json!({})),
            ),
            "task.complete" => self.task_complete(
                required_payload_value(object, "task", method_path)?,
                object.get("input").cloned().unwrap_or_else(|| json!({})),
            ),
            "task.handoff" => self.task_handoff(
                required_payload_value(object, "task", method_path)?,
                object.get("input").cloned().unwrap_or_else(|| json!({})),
            ),
            "task.acceptHandoff" => self.task_accept_handoff(
                required_payload_value(object, "task", method_path)?,
                object.get("input").cloned().unwrap_or_else(|| json!({})),
            ),
            "task.resume" => self.task_resume(
                required_payload_value(object, "task", method_path)?,
                object.get("input").cloned().unwrap_or_else(|| json!({})),
            ),
            "task.reclaim" => self.task_reclaim(
                required_payload_value(object, "task", method_path)?,
                object.get("input").cloned().unwrap_or_else(|| json!({})),
            ),
            _ => Err(anyhow!("unsupported compiler write method `{method_path}`")),
        }
    }

    pub(crate) fn declare_work(&self, input: Value) -> Result<Value> {
        let input = Value::Object(expect_object(input, "prism.work.declare")?);
        let args: PrismDeclareWorkArgs = serde_json::from_value(input.clone())
            .map_err(|error| anyhow!("`prism.work.declare` arguments invalid: {error}"))?;
        if args.title.trim().is_empty() {
            return Err(anyhow!(
                "`prism.work.declare` requires a non-empty `title`"
            ));
        }
        let mut state = self.state.lock().expect("code mutation lock poisoned");
        let handle_id = format!("work-handle:{}", state.next_work_handle);
        state.next_work_handle += 1;
        let preview = json!({
            "id": format!("work_{}", state.next_work_handle - 1),
            "title": input.get("title").cloned().unwrap_or(Value::Null),
            "summary": input.get("summary").cloned().unwrap_or(Value::Null),
            "kind": input.get("kind").cloned().unwrap_or(Value::Null),
            "provisional": true,
        });
        state.work_handles.insert(
            handle_id.clone(),
            DeferredHandleState {
                client_id: None,
                current: preview.clone(),
            },
        );
        self.record_write_effect(
            &mut state,
            "prism.work.declare",
            StructuredWriteOperationKind::Direct(StructuredDirectWriteOp::DeclareWork {
                handle_id: handle_id.clone(),
                input,
            }),
        )?;
        Ok(generic_handle_with_preview(
            &handle_id,
            WORK_HANDLE_KIND,
            &preview,
        ))
    }

    pub(crate) fn claim_acquire(&self, input: Value) -> Result<Value> {
        let input = expect_object(input, "prism.claim.acquire")?;
        let task_handle_id = input
            .get("coordinationTaskId")
            .cloned()
            .map(|task| self.ensure_task_handle_from_value(task, "prism.claim.acquire", false))
            .transpose()?;
        let mut state = self.state.lock().expect("code mutation lock poisoned");
        let handle_id = format!("claim-handle:{}", state.next_claim_handle);
        state.next_claim_handle += 1;
        let client_claim_id = format!("claim_{}", state.next_client_claim);
        state.next_client_claim += 1;
        let coordination_task_id = task_handle_id
            .as_ref()
            .and_then(|handle_id| state.task_handles.get(handle_id))
            .map(CurrentPreview::current_preview)
            .and_then(preview_id)
            .map(|id| Value::String(id.to_string()))
            .unwrap_or_else(|| {
                input.get("coordinationTaskId")
                    .cloned()
                    .unwrap_or(Value::Null)
            });
        let preview = json!({
            "id": format!("claim_{}", state.next_claim_handle - 1),
            "status": "Active",
            "anchors": input.get("anchors").cloned().unwrap_or(Value::Array(Vec::new())),
            "coordinationTaskId": coordination_task_id,
            "capability": input.get("capability").cloned().unwrap_or(Value::Null),
            "mode": input.get("mode").cloned().unwrap_or(Value::Null),
            "provisional": true,
        });
        state.claim_handles.insert(
            handle_id.clone(),
            DeferredHandleState {
                client_id: Some(client_claim_id.clone()),
                current: preview.clone(),
            },
        );
        self.record_write_effect(
            &mut state,
            "prism.claim.acquire",
            StructuredWriteOperationKind::Coordination(CoordinationWriteOp::ClaimAcquire {
                client_claim_id,
                task_handle_id,
                anchors: input
                    .get("anchors")
                    .cloned()
                    .unwrap_or(Value::Array(Vec::new())),
                capability: input.get("capability").cloned().unwrap_or(Value::Null),
                mode: input.get("mode").cloned(),
                ttl_seconds: input.get("ttlSeconds").and_then(Value::as_u64),
                agent: optional_string(&input, "agent"),
            }),
        )?;
        Ok(generic_handle_with_preview(
            &handle_id,
            CLAIM_HANDLE_KIND,
            &preview,
        ))
    }

    pub(crate) fn claim_renew(&self, claim: Value, input: Value) -> Result<Value> {
        let input = expect_object(input, "prism.claim.renew")?;
        let claim_handle_id = self.ensure_claim_handle_from_value(claim, "prism.claim.renew")?;
        let mut state = self.state.lock().expect("code mutation lock poisoned");
        self.record_write_effect(
            &mut state,
            "prism.claim.renew",
            StructuredWriteOperationKind::Coordination(CoordinationWriteOp::ClaimRenew {
                claim_handle_id: claim_handle_id.clone(),
                ttl_seconds: input.get("ttlSeconds").and_then(Value::as_u64),
            }),
        )?;
        let preview = state
            .claim_handles
            .get(&claim_handle_id)
            .map(|state| state.current.clone())
            .ok_or_else(|| anyhow!("unknown claim handle `{claim_handle_id}`"))?;
        Ok(generic_handle_with_preview(
            &claim_handle_id,
            CLAIM_HANDLE_KIND,
            &preview,
        ))
    }

    pub(crate) fn claim_release(&self, claim: Value) -> Result<Value> {
        let claim_handle_id = self.ensure_claim_handle_from_value(claim, "prism.claim.release")?;
        let mut state = self.state.lock().expect("code mutation lock poisoned");
        self.record_write_effect(
            &mut state,
            "prism.claim.release",
            StructuredWriteOperationKind::Coordination(CoordinationWriteOp::ClaimRelease {
                claim_handle_id: claim_handle_id.clone(),
            }),
        )?;
        if let Some(preview) = state
            .claim_handles
            .get_mut(&claim_handle_id)
            .and_then(|entry| entry.current.as_object_mut())
        {
            preview.insert("status".to_string(), Value::String("Released".to_string()));
        }
        let preview = state
            .claim_handles
            .get(&claim_handle_id)
            .map(|state| state.current.clone())
            .ok_or_else(|| anyhow!("unknown claim handle `{claim_handle_id}`"))?;
        Ok(generic_handle_with_preview(
            &claim_handle_id,
            CLAIM_HANDLE_KIND,
            &preview,
        ))
    }

    pub(crate) fn artifact_propose(&self, input: Value) -> Result<Value> {
        let input = expect_object(input, "prism.artifact.propose")?;
        let task_handle_id = input
            .get("taskId")
            .cloned()
            .map(|task| self.ensure_task_handle_from_value(task, "prism.artifact.propose", false))
            .transpose()?
            .ok_or_else(|| anyhow!("`prism.artifact.propose` requires `taskId`"))?;
        let mut state = self.state.lock().expect("code mutation lock poisoned");
        let handle_id = format!("artifact-handle:{}", state.next_artifact_handle);
        state.next_artifact_handle += 1;
        let client_artifact_id = format!("artifact_{}", state.next_client_artifact);
        state.next_client_artifact += 1;
        let task_id = state
            .task_handles
            .get(&task_handle_id)
            .map(CurrentPreview::current_preview)
            .and_then(preview_id)
            .map(|id| Value::String(id.to_string()))
            .unwrap_or_else(|| input.get("taskId").cloned().unwrap_or(Value::Null));
        let preview = json!({
            "id": format!("artifact_{}", state.next_artifact_handle - 1),
            "status": "Proposed",
            "taskId": task_id,
            "diffRef": input.get("diffRef").cloned().unwrap_or(Value::Null),
            "provisional": true,
        });
        state.artifact_handles.insert(
            handle_id.clone(),
            DeferredHandleState {
                client_id: Some(client_artifact_id.clone()),
                current: preview.clone(),
            },
        );
        self.record_write_effect(
            &mut state,
            "prism.artifact.propose",
            StructuredWriteOperationKind::Coordination(CoordinationWriteOp::ArtifactPropose {
                client_artifact_id,
                task_handle_id,
                artifact_requirement_id: optional_string(&input, "artifactRequirementId"),
                anchors: input.get("anchors").cloned(),
                diff_ref: optional_string(&input, "diffRef"),
                evidence: input.get("evidence").cloned(),
                required_validations: input.get("requiredValidations").cloned(),
                validated_checks: input.get("validatedChecks").cloned(),
                risk_score: input.get("riskScore").and_then(Value::as_f64),
            }),
        )?;
        Ok(generic_handle_with_preview(
            &handle_id,
            ARTIFACT_HANDLE_KIND,
            &preview,
        ))
    }

    pub(crate) fn artifact_supersede(&self, artifact: Value) -> Result<Value> {
        let artifact_handle_id =
            self.ensure_artifact_handle_from_value(artifact, "prism.artifact.supersede")?;
        let mut state = self.state.lock().expect("code mutation lock poisoned");
        self.record_write_effect(
            &mut state,
            "prism.artifact.supersede",
            StructuredWriteOperationKind::Coordination(CoordinationWriteOp::ArtifactSupersede {
                artifact_handle_id: artifact_handle_id.clone(),
            }),
        )?;
        if let Some(preview) = state
            .artifact_handles
            .get_mut(&artifact_handle_id)
            .and_then(|entry| entry.current.as_object_mut())
        {
            preview.insert("status".to_string(), Value::String("Superseded".to_string()));
        }
        let preview = state
            .artifact_handles
            .get(&artifact_handle_id)
            .map(|state| state.current.clone())
            .ok_or_else(|| anyhow!("unknown artifact handle `{artifact_handle_id}`"))?;
        Ok(generic_handle_with_preview(
            &artifact_handle_id,
            ARTIFACT_HANDLE_KIND,
            &preview,
        ))
    }

    pub(crate) fn artifact_review(&self, artifact: Value, input: Value) -> Result<Value> {
        let input = expect_object(input, "prism.artifact.review")?;
        let preview_input = input.clone();
        let artifact_handle_id =
            self.ensure_artifact_handle_from_value(artifact, "prism.artifact.review")?;
        let mut state = self.state.lock().expect("code mutation lock poisoned");
        self.record_write_effect(
            &mut state,
            "prism.artifact.review",
            StructuredWriteOperationKind::Coordination(CoordinationWriteOp::ArtifactReview {
                artifact_handle_id: artifact_handle_id.clone(),
                review_requirement_id: optional_string(&preview_input, "reviewRequirementId"),
                verdict: preview_input.get("verdict").cloned().unwrap_or(Value::Null),
                summary: required_string(&preview_input, "summary", "prism.artifact.review")?,
                required_validations: preview_input.get("requiredValidations").cloned(),
                validated_checks: preview_input.get("validatedChecks").cloned(),
                risk_score: preview_input.get("riskScore").and_then(Value::as_f64),
            }),
        )?;
        if let Some(preview) = state
            .artifact_handles
            .get_mut(&artifact_handle_id)
            .and_then(|entry| entry.current.as_object_mut())
        {
            let status = match preview
                .get("verdict")
                .and_then(Value::as_str)
                .or_else(|| preview_input.get("verdict").and_then(Value::as_str))
            {
                Some("approved") | Some("Approved") => "Approved",
                Some("changes_requested") | Some("ChangesRequested") => "InReview",
                Some("rejected") | Some("Rejected") => "Rejected",
                _ => "InReview",
            };
            preview.insert("status".to_string(), Value::String(status.to_string()));
            if let Some(verdict) = preview_input.get("verdict").cloned() {
                preview.insert("verdict".to_string(), verdict);
            }
        }
        let preview = state
            .artifact_handles
            .get(&artifact_handle_id)
            .map(|state| state.current.clone())
            .ok_or_else(|| anyhow!("unknown artifact handle `{artifact_handle_id}`"))?;
        Ok(generic_handle_with_preview(
            &artifact_handle_id,
            ARTIFACT_HANDLE_KIND,
            &preview,
        ))
    }

    pub(crate) fn task_handoff(&self, task: Value, input: Value) -> Result<Value> {
        let input = expect_object(input, "task.handoff")?;
        let task_handle_id = self.ensure_task_handle_from_value(task, "task.handoff", true)?;
        let summary = required_string(&input, "summary", "task.handoff")?;
        let to_agent =
            optional_string(&input, "toAgent").or_else(|| optional_string(&input, "to_agent"));
        let mut state = self.state.lock().expect("code mutation lock poisoned");
        self.record_write_effect(
            &mut state,
            "task.handoff",
            StructuredWriteOperationKind::Coordination(CoordinationWriteOp::TaskHandoff {
                task_handle_id: task_handle_id.clone(),
                summary,
                to_agent,
            }),
        )?;
        Ok(task_handle_with_preview(
            &task_handle_id,
            state.task_preview(&task_handle_id)?,
        ))
    }

    pub(crate) fn task_accept_handoff(&self, task: Value, input: Value) -> Result<Value> {
        let input = expect_object(input, "task.acceptHandoff")?;
        let task_handle_id =
            self.ensure_task_handle_from_value(task, "task.acceptHandoff", true)?;
        let agent = optional_string(&input, "agent");
        let mut state = self.state.lock().expect("code mutation lock poisoned");
        self.record_write_effect(
            &mut state,
            "task.acceptHandoff",
            StructuredWriteOperationKind::Coordination(CoordinationWriteOp::TaskAcceptHandoff {
                task_handle_id: task_handle_id.clone(),
                agent,
            }),
        )?;
        Ok(task_handle_with_preview(
            &task_handle_id,
            state.task_preview(&task_handle_id)?,
        ))
    }

    pub(crate) fn task_resume(&self, task: Value, input: Value) -> Result<Value> {
        let input = expect_object(input, "task.resume")?;
        let task_handle_id = self.ensure_task_handle_from_value(task, "task.resume", true)?;
        let agent = optional_string(&input, "agent");
        let mut state = self.state.lock().expect("code mutation lock poisoned");
        self.record_write_effect(
            &mut state,
            "task.resume",
            StructuredWriteOperationKind::Coordination(CoordinationWriteOp::TaskResume {
                task_handle_id: task_handle_id.clone(),
                agent,
            }),
        )?;
        Ok(task_handle_with_preview(
            &task_handle_id,
            state.task_preview(&task_handle_id)?,
        ))
    }

    pub(crate) fn task_reclaim(&self, task: Value, input: Value) -> Result<Value> {
        let input = expect_object(input, "task.reclaim")?;
        let task_handle_id = self.ensure_task_handle_from_value(task, "task.reclaim", true)?;
        let agent = optional_string(&input, "agent");
        let mut state = self.state.lock().expect("code mutation lock poisoned");
        self.record_write_effect(
            &mut state,
            "task.reclaim",
            StructuredWriteOperationKind::Coordination(CoordinationWriteOp::TaskReclaim {
                task_handle_id: task_handle_id.clone(),
                agent,
            }),
        )?;
        Ok(task_handle_with_preview(
            &task_handle_id,
            state.task_preview(&task_handle_id)?,
        ))
    }

    pub(crate) fn create_plan(&self, input: Value) -> Result<Value> {
        let input = expect_object(input, "prism.coordination.createPlan")?;
        let title = required_string(&input, "title", "prism.coordination.createPlan")?;
        let goal = optional_string(&input, "goal").unwrap_or_else(|| title.clone());
        let status = optional_string(&input, "status");
        let policy = input.get("policy").cloned();
        let scheduling = input.get("scheduling").cloned();
        let mut state = self.state.lock().expect("code mutation lock poisoned");
        let handle_id = format!("plan-handle:{}", state.next_plan_handle);
        state.next_plan_handle += 1;
        let client_plan_id = format!("plan_{}", state.next_client_plan);
        state.next_client_plan += 1;
        let preview = json!({
            "id": client_plan_id,
            "title": input.get("title").cloned().unwrap_or(Value::Null),
            "goal": goal,
            "status": status.clone().unwrap_or_else(|| "draft".to_string()),
            "provisional": true,
        });
        state.plan_handles.insert(
            handle_id.clone(),
            PlanHandleState::Created {
                client_plan_id: client_plan_id.clone(),
                committed_plan_id: None,
                current: preview.clone(),
            },
        );
        self.record_write_effect(
            &mut state,
            "prism.coordination.createPlan",
            StructuredWriteOperationKind::Coordination(CoordinationWriteOp::CreatePlan {
                client_plan_id,
                title,
                goal,
                status,
                policy,
                scheduling,
            }),
        )?;
        Ok(plan_handle_with_preview(&handle_id, &preview))
    }

    pub(crate) fn open_plan(&self, plan_id: String) -> Result<Value> {
        let mut state = self.state.lock().expect("code mutation lock poisoned");
        let plan_id = non_empty_string(plan_id, "prism.coordination.openPlan")?;
        let handle_id = format!("plan-handle:{}", state.next_plan_handle);
        state.next_plan_handle += 1;
        let preview = json!({
            "id": plan_id,
            "provisional": false,
        });
        state.plan_handles.insert(
            handle_id.clone(),
            PlanHandleState::Existing {
                plan_id,
                current: preview.clone(),
            },
        );
        Ok(plan_handle_with_preview(&handle_id, &preview))
    }

    pub(crate) fn plan_update(&self, plan: Value, input: Value) -> Result<Value> {
        let input = expect_object(input, "plan.update")?;
        let handle_id = plan_handle_id_from_value(&plan, "plan.update")?
            .ok_or_else(|| anyhow!("`plan.update` requires a plan handle"))?;
        let title = optional_string(&input, "title");
        let goal = optional_string(&input, "goal");
        let status = optional_string(&input, "status");
        let policy = input.get("policy").cloned();
        let scheduling = input.get("scheduling").cloned();
        if title.is_none()
            && goal.is_none()
            && status.is_none()
            && policy.is_none()
            && scheduling.is_none()
        {
            return Err(anyhow!(
                "`plan.update` requires at least one of `title`, `goal`, `status`, `policy`, or `scheduling`"
            ));
        }
        let mut state = self.state.lock().expect("code mutation lock poisoned");
        self.record_write_effect(
            &mut state,
            "plan.update",
            StructuredWriteOperationKind::Coordination(CoordinationWriteOp::PlanUpdate {
                plan_handle_id: handle_id.clone(),
                title: title.clone(),
                goal: goal.clone(),
                status: status.clone(),
                policy: policy.clone(),
                scheduling: scheduling.clone(),
            }),
        )?;
        let preview = state.plan_preview_mut(&handle_id)?;
        set_optional_string_field(preview, "title", title);
        set_optional_string_field(preview, "goal", goal);
        set_optional_string_field(preview, "status", status);
        set_optional_value_field(preview, "policy", policy);
        set_optional_value_field(preview, "scheduling", scheduling);
        Ok(plan_handle_with_preview(
            &handle_id,
            &Value::Object(preview.clone()),
        ))
    }

    pub(crate) fn plan_archive(&self, plan: Value) -> Result<Value> {
        let handle_id = plan_handle_id_from_value(&plan, "plan.archive")?
            .ok_or_else(|| anyhow!("`plan.archive` requires a plan handle"))?;
        let mut state = self.state.lock().expect("code mutation lock poisoned");
        self.record_write_effect(
            &mut state,
            "plan.archive",
            StructuredWriteOperationKind::Coordination(CoordinationWriteOp::PlanArchive {
                plan_handle_id: handle_id.clone(),
            }),
        )?;
        let preview = state.plan_preview_mut(&handle_id)?;
        preview.insert("status".to_string(), Value::String("archived".to_string()));
        Ok(plan_handle_with_preview(
            &handle_id,
            &Value::Object(preview.clone()),
        ))
    }

    pub(crate) fn open_task(&self, task_id: String) -> Result<Value> {
        let task_handle_id = self.ensure_task_handle_from_value(
            Value::String(task_id),
            "prism.coordination.openTask",
            true,
        )?;
        let state = self.state.lock().expect("code mutation lock poisoned");
        Ok(task_handle_with_preview(
            &task_handle_id,
            state.task_preview(&task_handle_id)?,
        ))
    }

    pub(crate) fn plan_add_task(&self, plan_handle_id: String, input: Value) -> Result<Value> {
        let input = expect_object(input, "plan.addTask")?;
        let title = required_string(&input, "title", "plan.addTask")?;
        let status = optional_string(&input, "status");
        let depends_on = input
            .get("dependsOn")
            .or_else(|| input.get("depends_on"))
            .cloned()
            .map(|value| self.task_handle_list_from_value(&value, "plan.addTask"))
            .transpose()?
            .unwrap_or_default();
        let mut state = self.state.lock().expect("code mutation lock poisoned");
        let handle_id = format!("task-handle:{}", state.next_task_handle);
        state.next_task_handle += 1;
        let client_task_id = format!("task_{}", state.next_client_task);
        state.next_client_task += 1;
        let preview = json!({
            "id": client_task_id,
            "planId": state
                .plan_handles
                .get(&plan_handle_id)
                .and_then(current_handle_id)
                .map(Value::String)
                .unwrap_or(Value::Null),
            "title": input.get("title").cloned().unwrap_or(Value::Null),
            "status": status.clone().unwrap_or_else(|| "proposed".to_string()),
            "provisional": true,
        });
        state.task_handles.insert(
            handle_id.clone(),
            TaskHandleState::Created {
                client_task_id: client_task_id.clone(),
                committed_task_id: None,
                current: preview.clone(),
            },
        );
        self.record_write_effect(
            &mut state,
            "plan.addTask",
            StructuredWriteOperationKind::Coordination(CoordinationWriteOp::CreateTask {
                plan_handle_id,
                client_task_id,
                title,
                status,
                depends_on,
                assignee: input.get("assignee").cloned(),
                anchors: input.get("anchors").cloned(),
                acceptance: input.get("acceptance").cloned(),
                artifact_requirements: input.get("artifactRequirements").cloned(),
                review_requirements: input.get("reviewRequirements").cloned(),
            }),
        )?;
        Ok(task_handle_with_preview(&handle_id, &preview))
    }

    pub(crate) fn task_update(&self, task: Value, input: Value) -> Result<Value> {
        self.task_update_with_method_path(task, input, "task.update")
    }

    fn task_update_with_method_path(
        &self,
        task: Value,
        input: Value,
        method_path: &str,
    ) -> Result<Value> {
        let input = expect_object(input, method_path)?;
        let handle_id = self.ensure_task_handle_from_value(task, method_path, true)?;
        let status = optional_string(&input, "status");
        let title = optional_string(&input, "title");
        let summary = optional_string_patch(&input, "summary", method_path)?;
        let assignee = optional_string_patch(&input, "assignee", method_path)?;
        let priority = optional_u8_patch(&input, "priority", method_path)?;
        let depends_on = input
            .get("dependsOn")
            .or_else(|| input.get("depends_on"))
            .cloned()
            .map(|value| self.task_handle_list_from_value(&value, method_path))
            .transpose()?;
        let has_object_field = |key: &str| input.contains_key(key);
        if status.is_none()
            && title.is_none()
            && summary.is_none()
            && assignee.is_none()
            && priority.is_none()
            && depends_on.is_none()
            && !has_object_field("anchors")
            && !has_object_field("acceptance")
            && !has_object_field("validationRefs")
            && !has_object_field("tags")
            && !has_object_field("artifactRequirements")
            && !has_object_field("reviewRequirements")
        {
            return Err(anyhow!(
                "`task.update` requires at least one supported field"
            ));
        }
        let mut state = self.state.lock().expect("code mutation lock poisoned");
        self.record_write_effect(
            &mut state,
            method_path,
            StructuredWriteOperationKind::Coordination(CoordinationWriteOp::TaskUpdate {
                task_handle_id: handle_id.clone(),
                status: status.clone(),
                title: title.clone(),
                summary: summary.clone(),
                assignee: assignee.clone(),
                priority: priority.clone(),
                depends_on: depends_on.clone(),
                anchors: input.get("anchors").cloned(),
                acceptance: input.get("acceptance").cloned(),
                validation_refs: input.get("validationRefs").cloned(),
                tags: input.get("tags").cloned(),
                artifact_requirements: input.get("artifactRequirements").cloned(),
                review_requirements: input.get("reviewRequirements").cloned(),
            }),
        )?;
        let preview = state.task_preview_mut(&handle_id)?;
        set_optional_string_field(preview, "status", status);
        set_optional_string_field(preview, "title", title);
        set_optional_patch_field(preview, "summary", summary);
        set_optional_patch_field(preview, "assignee", assignee);
        set_optional_patch_field(preview, "priority", priority);
        Ok(task_handle_with_preview(
            &handle_id,
            &Value::Object(preview.clone()),
        ))
    }

    pub(crate) fn task_depends_on(
        &self,
        task: Value,
        depends_on: Value,
        kind: Option<String>,
    ) -> Result<Value> {
        let task_handle_id = self.ensure_task_handle_from_value(task, "task.dependsOn", true)?;
        let depends_on_handle_id =
            self.ensure_task_handle_from_value(depends_on, "task.dependsOn", true)?;
        let mut state = self.state.lock().expect("code mutation lock poisoned");
        self.record_write_effect(
            &mut state,
            "task.dependsOn",
            StructuredWriteOperationKind::Coordination(CoordinationWriteOp::TaskDependsOn {
                task_handle_id,
                depends_on_handle_id,
                kind: kind.unwrap_or_else(|| "depends_on".to_string()),
            }),
        )?;
        Ok(Value::Null)
    }

    pub(crate) fn task_complete(&self, task: Value, input: Value) -> Result<Value> {
        let mut update = expect_object(input, "task.complete")?;
        update.insert("status".to_string(), Value::String("completed".to_string()));
        self.task_update_with_method_path(task, Value::Object(update), "task.complete")
    }

    pub(crate) fn finalize_result(&self, result: Value) -> Result<Value> {
        let mut state = self.state.lock().expect("code mutation lock poisoned");
        if state.transaction_plan.is_empty() {
            return Ok(result);
        }
        let transaction_plan = state.take_transaction_plan(&self.analyzed);
        if self.dry_run {
            return Ok(resolve_handles(result, &state));
        }
        let commit_payload =
            structured_compiler_commit_payload(&self.analyzed, &state, &transaction_plan)?;
        let commit = (self.coordination_executor)(commit_payload)?;
        reject_coordination_commit_if_needed(&commit)?;
        apply_coordination_commit(&mut state, &commit);
        apply_session_effect_results(&mut state, &commit);
        Ok(resolve_handles(result, &state))
    }

    pub(crate) fn overlay_plan_read(&self, plan_id: &str, base: Option<Value>) -> Option<Value> {
        let state = self.state.lock().expect("code mutation lock poisoned");
        overlay_plan_read_from_state(&state, plan_id, base)
    }

    pub(crate) fn overlay_task_read(&self, task_id: &str, base: Option<Value>) -> Option<Value> {
        let state = self.state.lock().expect("code mutation lock poisoned");
        overlay_task_read_from_state(&state, task_id, base)
    }

    pub(crate) fn overlay_plan_list(&self, base: Vec<Value>) -> Vec<Value> {
        let state = self.state.lock().expect("code mutation lock poisoned");
        overlay_plan_list_from_state(&state, base)
    }

    pub(crate) fn overlay_task_list(&self, base: Vec<Value>) -> Vec<Value> {
        let state = self.state.lock().expect("code mutation lock poisoned");
        overlay_task_list_from_state(&state, base)
    }

    pub(crate) fn overlay_plan_children(&self, plan_id: &str, base: Vec<Value>) -> Vec<Value> {
        let state = self.state.lock().expect("code mutation lock poisoned");
        overlay_plan_children_from_state(&state, plan_id, base)
    }

    pub(crate) fn overlay_artifact_list(
        &self,
        task_id: Option<&str>,
        plan_id: Option<&str>,
        base: Vec<Value>,
    ) -> Vec<Value> {
        let state = self.state.lock().expect("code mutation lock poisoned");
        overlay_artifact_list_from_state(&state, task_id, plan_id, base)
    }

    pub(crate) fn overlay_claim_list(&self, anchors: &[Value], base: Vec<Value>) -> Vec<Value> {
        let state = self.state.lock().expect("code mutation lock poisoned");
        overlay_claim_list_from_state(&state, anchors, base)
    }

    fn ensure_task_handle_from_value(
        &self,
        value: Value,
        method: &str,
        allow_provisional: bool,
    ) -> Result<String> {
        let mut state = self.state.lock().expect("code mutation lock poisoned");
        ensure_task_handle_from_value_with_state(&mut state, value, method, allow_provisional)
    }

    fn ensure_claim_handle_from_value(&self, value: Value, method: &str) -> Result<String> {
        let mut state = self.state.lock().expect("code mutation lock poisoned");
        if let Some(handle_id) = handle_id_from_value(&value, CLAIM_HANDLE_KIND)? {
            return Ok(handle_id);
        }
        let claim_id = existing_object_id_from_value(&value, method, "claim")?;
        let handle_id = format!("claim-handle:{}", state.next_claim_handle);
        state.next_claim_handle += 1;
        let preview = json!({
            "id": claim_id,
            "provisional": false,
        });
        state
            .claim_handles
            .insert(
                handle_id.clone(),
                DeferredHandleState {
                    client_id: None,
                    current: preview,
                },
            );
        Ok(handle_id)
    }

    fn ensure_artifact_handle_from_value(&self, value: Value, method: &str) -> Result<String> {
        let mut state = self.state.lock().expect("code mutation lock poisoned");
        if let Some(handle_id) = handle_id_from_value(&value, ARTIFACT_HANDLE_KIND)? {
            return Ok(handle_id);
        }
        let artifact_id = existing_object_id_from_value(&value, method, "artifact")?;
        let handle_id = format!("artifact-handle:{}", state.next_artifact_handle);
        state.next_artifact_handle += 1;
        let preview = json!({
            "id": artifact_id,
            "provisional": false,
        });
        state
            .artifact_handles
            .insert(
                handle_id.clone(),
                DeferredHandleState {
                    client_id: None,
                    current: preview,
                },
            );
        Ok(handle_id)
    }

    fn task_handle_list_from_value(&self, value: &Value, method: &str) -> Result<Vec<String>> {
        let values = value
            .as_array()
            .ok_or_else(|| anyhow!("`{method}` expects `dependsOn` to be an array"))?;
        values
            .iter()
            .cloned()
            .map(|entry| self.ensure_task_handle_from_value(entry, method, true))
            .collect()
    }

    fn record_write_effect(
        &self,
        state: &mut PrismCodeWriteState,
        method_path: &str,
        payload: StructuredWriteOperationKind,
    ) -> Result<()> {
        let metadata = self.metadata_for_write_method_path(method_path)?;
        state.transaction_plan.record_effect(
            &self.analyzed.ir,
            StructuredTransactionEffectMetadata {
                method_path: metadata.method_path,
                effect_id: metadata.effect_id,
                region_id: metadata.region_id,
                region_lineage: metadata.region_lineage,
                span: metadata.span,
            },
            payload,
        )?;
        Ok(())
    }

    #[cfg(test)]
    fn debug_transaction_plan(&self) -> StructuredTransactionPlan<StructuredWriteOperationKind> {
        self.state
            .lock()
            .expect("code mutation lock poisoned")
            .transaction_plan
            .clone()
    }

    fn metadata_for_write_method_path(&self, method_path: &str) -> Result<StructuredWriteMetadata> {
        let mut cursors = self
            .effect_cursors
            .lock()
            .expect("effect cursor lock poisoned");
        let index = cursors.entry(method_path.to_string()).or_insert(0);
        let matching_effects = self
            .analyzed
            .ir
            .effects
            .iter()
            .filter(|effect| {
                effect.method_path.as_deref() == Some(method_path)
                    && effect.kind == PrismProgramEffectKind::AuthoritativeWrite
            })
            .collect::<Vec<_>>();
        let effect = matching_effects
            .get(*index)
            .copied()
            .or_else(|| {
                matching_effects.last().copied().filter(|effect| {
                    matching_effects.len() == 1
                        || region_supports_repeated_effect_execution(&self.analyzed, effect.region_id)
                })
            })
            .ok_or_else(|| {
                anyhow!(
                    "compiler write lowering could not resolve authoritative effect metadata for `{method_path}`"
                )
            })?;
        if *index < matching_effects.len() {
            *index += 1;
        }
        Ok(StructuredWriteMetadata {
            method_path: method_path.to_string(),
            effect_id: Some(effect.id),
            region_id: effect.region_id,
            region_lineage: region_lineage(&self.analyzed, effect.region_id),
            span: Some(effect.span.clone()),
        })
    }
}

fn required_payload_value(
    payload: &Map<String, Value>,
    key: &str,
    method: &str,
) -> Result<Value> {
    payload
        .get(key)
        .cloned()
        .ok_or_else(|| anyhow!("`{method}` requires `{key}`"))
}

fn required_payload_string(
    payload: &Map<String, Value>,
    key: &str,
    method: &str,
) -> Result<String> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| anyhow!("`{method}` requires `{key}`"))
}

fn optional_payload_string(payload: &Map<String, Value>, key: &str) -> Option<String> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

impl PrismCodeWriteState {
    fn new(analyzed: &AnalyzedPrismProgram) -> Self {
        Self {
            next_plan_handle: 0,
            next_task_handle: 0,
            next_claim_handle: 0,
            next_artifact_handle: 0,
            next_work_handle: 0,
            next_client_plan: 0,
            next_client_task: 0,
            next_client_claim: 0,
            next_client_artifact: 0,
            transaction_plan: StructuredTransactionPlan::new(&analyzed.ir),
            plan_handles: BTreeMap::new(),
            task_handles: BTreeMap::new(),
            claim_handles: BTreeMap::new(),
            artifact_handles: BTreeMap::new(),
            work_handles: BTreeMap::new(),
        }
    }

    fn take_transaction_plan(
        &mut self,
        analyzed: &AnalyzedPrismProgram,
    ) -> StructuredTransactionPlan<StructuredWriteOperationKind> {
        std::mem::replace(
            &mut self.transaction_plan,
            StructuredTransactionPlan::new(&analyzed.ir),
        )
    }

    fn plan_preview_mut(&mut self, handle_id: &str) -> Result<&mut Map<String, Value>> {
        match self.plan_handles.get_mut(handle_id) {
            Some(
                PlanHandleState::Created { current, .. }
                | PlanHandleState::Existing { current, .. },
            ) => current.as_object_mut().ok_or_else(|| {
                anyhow!("plan handle `{handle_id}` does not contain an object preview")
            }),
            None => Err(anyhow!("unknown plan handle `{handle_id}`")),
        }
    }

    fn task_preview_mut(&mut self, handle_id: &str) -> Result<&mut Map<String, Value>> {
        match self.task_handles.get_mut(handle_id) {
            Some(
                TaskHandleState::Created { current, .. }
                | TaskHandleState::Existing { current, .. },
            ) => current.as_object_mut().ok_or_else(|| {
                anyhow!("task handle `{handle_id}` does not contain an object preview")
            }),
            None => Err(anyhow!("unknown task handle `{handle_id}`")),
        }
    }

    fn task_preview(&self, handle_id: &str) -> Result<&Value> {
        match self.task_handles.get(handle_id) {
            Some(
                TaskHandleState::Created { current, .. }
                | TaskHandleState::Existing { current, .. },
            ) => Ok(current),
            None => Err(anyhow!("unknown task handle `{handle_id}`")),
        }
    }

    fn update_task_handle_current(&mut self, handle_id: &str, value: Value) {
        match self.task_handles.get_mut(handle_id) {
            Some(
                TaskHandleState::Created { current, .. }
                | TaskHandleState::Existing { current, .. },
            ) => {
                *current = value;
            }
            None => {}
        }
    }
}

fn current_handle_id<T>(state: &T) -> Option<String>
where
    T: CurrentPreview,
{
    state.current_preview()
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
}

trait CurrentPreview {
    fn current_preview(&self) -> &Value;
}

impl CurrentPreview for PlanHandleState {
    fn current_preview(&self) -> &Value {
        match self {
            Self::Created { current, .. } | Self::Existing { current, .. } => current,
        }
    }
}

impl CurrentPreview for TaskHandleState {
    fn current_preview(&self) -> &Value {
        match self {
            Self::Created { current, .. } | Self::Existing { current, .. } => current,
        }
    }
}

impl CurrentPreview for DeferredHandleState {
    fn current_preview(&self) -> &Value {
        &self.current
    }
}

fn overlay_plan_read_from_state(
    state: &PrismCodeWriteState,
    plan_id: &str,
    base: Option<Value>,
) -> Option<Value> {
    let overlay = state
        .plan_handles
        .values()
        .map(CurrentPreview::current_preview)
        .find(|preview| preview_id(preview).is_some_and(|id| id == plan_id))
        .cloned();
    match (base, overlay) {
        (Some(base), Some(overlay)) => Some(merge_preview_overlay(base, &overlay)),
        (Some(base), None) => Some(base),
        (None, Some(overlay)) => Some(overlay),
        (None, None) => None,
    }
}

fn overlay_task_read_from_state(
    state: &PrismCodeWriteState,
    task_id: &str,
    base: Option<Value>,
) -> Option<Value> {
    let overlay = state
        .task_handles
        .values()
        .map(CurrentPreview::current_preview)
        .find(|preview| preview_id(preview).is_some_and(|id| id == task_id))
        .cloned();
    match (base, overlay) {
        (Some(base), Some(overlay)) => Some(merge_preview_overlay(base, &overlay)),
        (Some(base), None) => Some(base),
        (None, Some(overlay)) => Some(overlay),
        (None, None) => None,
    }
}

fn overlay_plan_list_from_state(state: &PrismCodeWriteState, base: Vec<Value>) -> Vec<Value> {
    let overlays = state
        .plan_handles
        .values()
        .map(CurrentPreview::current_preview)
        .cloned()
        .collect::<Vec<_>>();
    overlay_collection_by_id(base, overlays)
}

fn overlay_task_list_from_state(state: &PrismCodeWriteState, base: Vec<Value>) -> Vec<Value> {
    let overlays = state
        .task_handles
        .values()
        .map(CurrentPreview::current_preview)
        .cloned()
        .collect::<Vec<_>>();
    overlay_collection_by_id(base, overlays)
}

fn overlay_plan_children_from_state(
    state: &PrismCodeWriteState,
    plan_id: &str,
    base: Vec<Value>,
) -> Vec<Value> {
    let overlays = state
        .task_handles
        .values()
        .map(CurrentPreview::current_preview)
        .filter(|preview| {
            preview
                .get("planId")
                .and_then(Value::as_str)
                .is_some_and(|candidate| candidate == plan_id)
        })
        .cloned()
        .collect::<Vec<_>>();
    overlay_collection_by_id(base, overlays)
}

fn overlay_artifact_list_from_state(
    state: &PrismCodeWriteState,
    task_id: Option<&str>,
    plan_id: Option<&str>,
    base: Vec<Value>,
) -> Vec<Value> {
    let overlays = state
        .artifact_handles
        .values()
        .map(CurrentPreview::current_preview)
        .filter(|preview| matches_artifact_scope(state, preview, task_id, plan_id))
        .cloned()
        .collect::<Vec<_>>();
    overlay_collection_by_id(base, overlays)
}

fn overlay_claim_list_from_state(
    state: &PrismCodeWriteState,
    anchors: &[Value],
    base: Vec<Value>,
) -> Vec<Value> {
    let overlays = state
        .claim_handles
        .values()
        .map(CurrentPreview::current_preview)
        .filter(|preview| {
            preview
                .get("anchors")
                .and_then(Value::as_array)
                .is_some_and(|candidate| candidate == anchors)
        })
        .cloned()
        .collect::<Vec<_>>();
    overlay_collection_by_id(base, overlays)
}

fn matches_artifact_scope(
    state: &PrismCodeWriteState,
    preview: &Value,
    task_id: Option<&str>,
    plan_id: Option<&str>,
) -> bool {
    let artifact_task_id = preview.get("taskId").and_then(Value::as_str);
    if let Some(task_id) = task_id {
        return artifact_task_id.is_some_and(|candidate| candidate == task_id);
    }
    if let Some(plan_id) = plan_id {
        let Some(task_id) = artifact_task_id else {
            return false;
        };
        return state
            .task_handles
            .values()
            .map(CurrentPreview::current_preview)
            .find(|task| preview_id(task).is_some_and(|candidate| candidate == task_id))
            .and_then(|task| task.get("planId").and_then(Value::as_str))
            .is_some_and(|candidate| candidate == plan_id);
    }
    true
}

fn overlay_collection_by_id(base: Vec<Value>, overlays: Vec<Value>) -> Vec<Value> {
    let mut entries = base;
    for overlay in overlays {
        let Some(id) = preview_id(&overlay).map(str::to_string) else {
            continue;
        };
        if let Some(existing) = entries
            .iter_mut()
            .find(|entry| preview_id(entry).is_some_and(|candidate| candidate == id))
        {
            *existing = merge_preview_overlay(existing.clone(), &overlay);
        } else {
            entries.push(overlay);
        }
    }
    entries
}

fn merge_preview_overlay(base: Value, overlay: &Value) -> Value {
    let Some(base_object) = base.as_object() else {
        return overlay.clone();
    };
    let Some(overlay_object) = overlay.as_object() else {
        return base;
    };
    let mut merged = base_object.clone();
    for (key, value) in overlay_object {
        merged.insert(key.clone(), value.clone());
    }
    Value::Object(merged)
}

fn preview_id(value: &Value) -> Option<&str> {
    value.get("id").and_then(Value::as_str)
}

fn region_lineage(
    analyzed: &AnalyzedPrismProgram,
    mut region_id: PrismProgramRegionId,
) -> Vec<PrismProgramRegionId> {
    let mut lineage = Vec::new();
    loop {
        lineage.push(region_id);
        let Some(parent) = analyzed.ir.regions[region_id].parent else {
            break;
        };
        region_id = parent;
    }
    lineage.reverse();
    lineage
}

fn region_supports_repeated_effect_execution(
    analyzed: &AnalyzedPrismProgram,
    mut region_id: PrismProgramRegionId,
) -> bool {
    loop {
        match analyzed.ir.regions[region_id].control {
            super::program_ir::PrismProgramRegionControl::Loop { .. }
            | super::program_ir::PrismProgramRegionControl::Parallel { .. }
            | super::program_ir::PrismProgramRegionControl::Reduction { .. }
            | super::program_ir::PrismProgramRegionControl::Competition { .. }
            | super::program_ir::PrismProgramRegionControl::CallbackBoundary { .. } => {
                return true;
            }
            _ => {}
        }
        let Some(parent) = analyzed.ir.regions[region_id].parent else {
            return false;
        };
        region_id = parent;
    }
}

fn coordination_intent_metadata(
    analyzed: &AnalyzedPrismProgram,
    transaction_plan: &StructuredTransactionPlan<StructuredWriteOperationKind>,
) -> Option<Value> {
    let mut regions = Vec::new();
    let mut seen_regions = std::collections::BTreeSet::new();
    let mut effects = Vec::new();
    for effect_id in transaction_plan.ordered_effect_ids() {
        let effect = &transaction_plan.effects[effect_id];
        let StructuredWriteOperationKind::Coordination(_) = &effect.payload else {
            continue;
        };
        for region_id in &effect.metadata.region_lineage {
            if seen_regions.insert(*region_id) {
                let region = &analyzed.ir.regions[*region_id];
                regions.push(json!({
                    "regionId": region.id,
                    "parentRegionId": region.parent,
                    "control": region.control,
                    "span": region.span,
                    "exitModes": region.exit_modes,
                }));
            }
        }
        effects.push(json!({
            "methodPath": effect.metadata.method_path,
            "effectId": effect.metadata.effect_id,
            "regionId": effect.metadata.region_id,
            "regionLineage": effect.metadata.region_lineage,
            "span": effect.metadata.span,
        }));
    }
    if effects.is_empty() {
        return None;
    }
    Some(json!({
        "compilerLowering": {
            "mode": "structured_transaction",
            "effects": effects,
            "regions": regions,
        }
    }))
}

fn structured_compiler_commit_payload(
    analyzed: &AnalyzedPrismProgram,
    state: &PrismCodeWriteState,
    transaction_plan: &StructuredTransactionPlan<StructuredWriteOperationKind>,
) -> Result<Value> {
    let mut payload = Map::new();
    if let Some(intent_metadata) = coordination_intent_metadata(analyzed, transaction_plan) {
        payload.insert("intentMetadata".to_string(), intent_metadata);
    }
    if let Some(structured_transaction) =
        compiler_structured_transaction_payload(analyzed, state, transaction_plan)?
    {
        payload.insert("structuredTransaction".to_string(), structured_transaction);
    }
    Ok(Value::Object(payload))
}

fn compiler_structured_transaction_payload(
    analyzed: &AnalyzedPrismProgram,
    state: &PrismCodeWriteState,
    transaction_plan: &StructuredTransactionPlan<StructuredWriteOperationKind>,
) -> Result<Option<Value>> {
    let effect_ids = transaction_plan.ordered_effect_ids();
    if effect_ids.is_empty() {
        return Ok(None);
    }
    let effect_id_set = effect_ids
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    let mut region_ids = std::collections::BTreeSet::new();
    for effect_id in &effect_ids {
        let effect = &transaction_plan.effects[*effect_id];
        for region_id in &effect.metadata.region_lineage {
            region_ids.insert(*region_id);
        }
    }
    let root_region_id = effect_ids
        .first()
        .and_then(|effect_id| {
            transaction_plan.effects[*effect_id]
                .metadata
                .region_lineage
                .first()
                .copied()
        })
        .unwrap_or(analyzed.ir.root_region_id);
    let effects = effect_ids
        .iter()
        .map(|effect_id| {
            let effect = &transaction_plan.effects[*effect_id];
            let metadata = json!({
                "methodPath": effect.metadata.method_path,
                "effectId": effect.metadata.effect_id,
                "regionId": effect.metadata.region_id,
                "regionLineage": effect.metadata.region_lineage,
                "span": effect.metadata.span,
            });
            Ok(match &effect.payload {
                StructuredWriteOperationKind::Coordination(operation) => json!({
                    "id": effect.id,
                    "kind": "coordination",
                    "metadata": metadata,
                    "payload": lower_coordination_op(state, &operation)?,
                }),
                StructuredWriteOperationKind::Direct(StructuredDirectWriteOp::DeclareWork {
                    handle_id,
                    input,
                }) => json!({
                    "id": effect.id,
                    "kind": "declare_work",
                    "metadata": metadata,
                    "handleId": handle_id,
                    "input": input,
                }),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let regions = region_ids
        .iter()
        .map(|region_id| {
            let region = transaction_plan
                .region(*region_id)
                .expect("transaction plan region should exist for emitted region id");
            let child_region_ids = region
                .child_region_ids
                .iter()
                .copied()
                .filter(|child_id| region_ids.contains(child_id))
                .collect::<Vec<_>>();
            let effect_ids = region
                .effect_ids
                .iter()
                .copied()
                .filter(|effect_id| effect_id_set.contains(effect_id))
                .collect::<Vec<_>>();
            let members = region
                .members
                .iter()
                .filter_map(|member| match member {
                    super::transaction_plan::StructuredTransactionRegionMember::Region(child_id)
                        if region_ids.contains(child_id) =>
                    {
                        Some(json!({ "kind": "region", "id": child_id }))
                    }
                    super::transaction_plan::StructuredTransactionRegionMember::Effect(effect_id)
                        if effect_id_set.contains(effect_id) =>
                    {
                        Some(json!({ "kind": "effect", "id": effect_id }))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            let ir_region = &analyzed.ir.regions[*region_id];
            json!({
                "regionId": region.region_id,
                "parentRegionId": region.parent_region_id.filter(|parent| region_ids.contains(parent)),
                "control": region.control,
                "span": region.span,
                "exitModes": ir_region.exit_modes,
                "childRegionIds": child_region_ids,
                "effectIds": effect_ids,
                "members": members,
            })
        })
        .collect::<Vec<_>>();
    Ok(Some(json!({
        "rootRegionId": root_region_id,
        "regions": regions,
        "effects": effects,
    })))
}

fn lower_coordination_op(
    state: &PrismCodeWriteState,
    operation: &CoordinationWriteOp,
) -> Result<Value> {
    Ok(match operation {
        CoordinationWriteOp::CreatePlan {
            client_plan_id,
            title,
            goal,
            status,
            policy,
            scheduling,
            ..
        } => json!({
            "kind": "plan_create",
            "input": {
                "clientPlanId": client_plan_id,
                "title": title,
                "goal": goal,
                "status": status,
                "policy": policy,
                "scheduling": scheduling,
            }
        }),
        CoordinationWriteOp::PlanUpdate {
            plan_handle_id,
            title,
            goal,
            status,
            policy,
            scheduling,
        } => json!({
            "kind": "plan_update",
            "input": {
                "plan": plan_ref_value(state, plan_handle_id)?,
                "title": title,
                "goal": goal,
                "status": status,
                "policy": policy,
                "scheduling": scheduling,
            }
        }),
        CoordinationWriteOp::PlanArchive { plan_handle_id } => json!({
            "kind": "plan_archive",
            "input": {
                "plan": plan_ref_value(state, plan_handle_id)?,
            }
        }),
        CoordinationWriteOp::CreateTask {
            plan_handle_id,
            client_task_id,
            title,
            status,
            depends_on,
            assignee,
            anchors,
            acceptance,
            artifact_requirements,
            review_requirements,
            ..
        } => {
            let mut input = Map::new();
            input.insert(
                "clientTaskId".to_string(),
                Value::String(client_task_id.clone()),
            );
            input.insert("plan".to_string(), plan_ref_value(state, plan_handle_id)?);
            input.insert("title".to_string(), Value::String(title.clone()));
            if let Some(status) = status {
                input.insert("status".to_string(), Value::String(status.clone()));
            }
            if !depends_on.is_empty() {
                input.insert(
                    "dependsOn".to_string(),
                    Value::Array(task_ref_values(state, depends_on)?),
                );
            }
            insert_value_if_present(&mut input, "assignee", assignee.clone());
            insert_value_if_present(&mut input, "anchors", anchors.clone());
            insert_value_if_present(&mut input, "acceptance", acceptance.clone());
            insert_value_if_present(
                &mut input,
                "artifactRequirements",
                artifact_requirements.clone(),
            );
            insert_value_if_present(
                &mut input,
                "reviewRequirements",
                review_requirements.clone(),
            );
            json!({
                "kind": "task_create",
                "input": input,
            })
        }
        CoordinationWriteOp::TaskUpdate {
            task_handle_id,
            status,
            title,
            summary,
            assignee,
            priority,
            depends_on,
            anchors,
            acceptance,
            validation_refs,
            tags,
            artifact_requirements,
            review_requirements,
        } => {
            let mut input = Map::new();
            input.insert(
                "task".to_string(),
                task_ref_value(state, task_handle_id, "task.update")?,
            );
            insert_string_if_present(&mut input, "status", status.clone());
            insert_string_if_present(&mut input, "title", title.clone());
            insert_value_if_present(&mut input, "summary", summary.clone());
            insert_value_if_present(&mut input, "assignee", assignee.clone());
            insert_value_if_present(&mut input, "priority", priority.clone());
            if let Some(depends_on) = depends_on {
                input.insert(
                    "dependsOn".to_string(),
                    Value::Array(task_ref_values(state, depends_on)?),
                );
            }
            insert_value_if_present(&mut input, "anchors", anchors.clone());
            insert_value_if_present(&mut input, "acceptance", acceptance.clone());
            insert_value_if_present(&mut input, "validationRefs", validation_refs.clone());
            insert_value_if_present(&mut input, "tags", tags.clone());
            insert_value_if_present(
                &mut input,
                "artifactRequirements",
                artifact_requirements.clone(),
            );
            insert_value_if_present(
                &mut input,
                "reviewRequirements",
                review_requirements.clone(),
            );
            json!({
                "kind": "task_update",
                "input": input,
            })
        }
        CoordinationWriteOp::TaskDependsOn {
            task_handle_id,
            depends_on_handle_id,
            kind,
        } => json!({
            "kind": "dependency_create",
            "input": {
                "task": task_ref_value(state, task_handle_id, "task.dependsOn")?,
                "dependsOn": task_ref_value(state, depends_on_handle_id, "task.dependsOn")?,
                "kind": kind,
            }
        }),
        CoordinationWriteOp::ClaimAcquire {
            client_claim_id,
            task_handle_id,
            anchors,
            capability,
            mode,
            ttl_seconds,
            agent,
        } => {
            let mut input = Map::new();
            input.insert(
                "clientClaimId".to_string(),
                Value::String(client_claim_id.clone()),
            );
            if let Some(task_handle_id) = task_handle_id {
                input.insert("task".to_string(), task_ref_value(state, task_handle_id, "prism.claim.acquire")?);
            }
            input.insert("anchors".to_string(), anchors.clone());
            input.insert("capability".to_string(), capability.clone());
            insert_value_if_present(&mut input, "mode", mode.clone());
            if let Some(ttl_seconds) = ttl_seconds {
                input.insert("ttlSeconds".to_string(), Value::from(*ttl_seconds));
            }
            if let Some(agent) = agent {
                input.insert("agent".to_string(), Value::String(agent.clone()));
            }
            json!({
                "kind": "claim_acquire",
                "input": input,
            })
        }
        CoordinationWriteOp::ClaimRenew {
            claim_handle_id,
            ttl_seconds,
        } => {
            let mut input = Map::new();
            input.insert(
                "claim".to_string(),
                claim_ref_value(state, claim_handle_id, "prism.claim.renew")?,
            );
            if let Some(ttl_seconds) = ttl_seconds {
                input.insert("ttlSeconds".to_string(), Value::from(*ttl_seconds));
            }
            json!({
                "kind": "claim_renew",
                "input": input,
            })
        }
        CoordinationWriteOp::ClaimRelease { claim_handle_id } => json!({
            "kind": "claim_release",
            "input": {
                "claim": claim_ref_value(state, claim_handle_id, "prism.claim.release")?,
            }
        }),
        CoordinationWriteOp::ArtifactPropose {
            client_artifact_id,
            task_handle_id,
            artifact_requirement_id,
            anchors,
            diff_ref,
            evidence,
            required_validations,
            validated_checks,
            risk_score,
        } => {
            let mut input = Map::new();
            input.insert(
                "clientArtifactId".to_string(),
                Value::String(client_artifact_id.clone()),
            );
            input.insert(
                "task".to_string(),
                task_ref_value(state, task_handle_id, "prism.artifact.propose")?,
            );
            insert_string_if_present(
                &mut input,
                "artifactRequirementId",
                artifact_requirement_id.clone(),
            );
            insert_value_if_present(&mut input, "anchors", anchors.clone());
            insert_string_if_present(&mut input, "diffRef", diff_ref.clone());
            insert_value_if_present(&mut input, "evidence", evidence.clone());
            insert_value_if_present(
                &mut input,
                "requiredValidations",
                required_validations.clone(),
            );
            insert_value_if_present(&mut input, "validatedChecks", validated_checks.clone());
            if let Some(risk_score) = risk_score {
                input.insert("riskScore".to_string(), Value::from(*risk_score));
            }
            json!({
                "kind": "artifact_propose",
                "input": input,
            })
        }
        CoordinationWriteOp::ArtifactSupersede { artifact_handle_id } => json!({
            "kind": "artifact_supersede",
            "input": {
                "artifact": artifact_ref_value(state, artifact_handle_id, "prism.artifact.supersede")?,
            }
        }),
        CoordinationWriteOp::ArtifactReview {
            artifact_handle_id,
            review_requirement_id,
            verdict,
            summary,
            required_validations,
            validated_checks,
            risk_score,
        } => {
            let mut input = Map::new();
            input.insert(
                "artifact".to_string(),
                artifact_ref_value(state, artifact_handle_id, "prism.artifact.review")?,
            );
            insert_string_if_present(
                &mut input,
                "reviewRequirementId",
                review_requirement_id.clone(),
            );
            input.insert("verdict".to_string(), verdict.clone());
            input.insert("summary".to_string(), Value::String(summary.clone()));
            insert_value_if_present(
                &mut input,
                "requiredValidations",
                required_validations.clone(),
            );
            insert_value_if_present(&mut input, "validatedChecks", validated_checks.clone());
            if let Some(risk_score) = risk_score {
                input.insert("riskScore".to_string(), Value::from(*risk_score));
            }
            json!({
                "kind": "artifact_review",
                "input": input,
            })
        }
        CoordinationWriteOp::TaskHandoff {
            task_handle_id,
            summary,
            to_agent,
        } => json!({
            "kind": "task_handoff",
            "input": {
                "task": task_ref_value(state, task_handle_id, "task.handoff")?,
                "summary": summary,
                "toAgent": to_agent,
            }
        }),
        CoordinationWriteOp::TaskAcceptHandoff {
            task_handle_id,
            agent,
        } => json!({
            "kind": "task_handoff_accept",
            "input": {
                "task": task_ref_value(state, task_handle_id, "task.acceptHandoff")?,
                "agent": agent,
            }
        }),
        CoordinationWriteOp::TaskResume {
            task_handle_id,
            agent,
        } => json!({
            "kind": "task_resume",
            "input": {
                "task": task_ref_value(state, task_handle_id, "task.resume")?,
                "agent": agent,
            }
        }),
        CoordinationWriteOp::TaskReclaim {
            task_handle_id,
            agent,
        } => json!({
            "kind": "task_reclaim",
            "input": {
                "task": task_ref_value(state, task_handle_id, "task.reclaim")?,
                "agent": agent,
            }
        }),
    })
}

fn apply_coordination_commit(state: &mut PrismCodeWriteState, commit: &Value) {
    for plan_state in state.plan_handles.values_mut() {
        match plan_state {
            PlanHandleState::Created {
                client_plan_id,
                committed_plan_id,
                current,
            } => {
                let resolved_plan_id = commit
                    .get("planIdsByClientId")
                    .and_then(|mapping| mapping.get(client_plan_id.as_str()))
                    .and_then(Value::as_str)
                    .map(str::to_string);
                if let Some(plan_id) = resolved_plan_id {
                    *committed_plan_id = Some(plan_id.clone());
                    if let Some(view) = committed_plan_view(commit, &plan_id) {
                        *current = view;
                    } else {
                        *current = json!({ "id": plan_id });
                    }
                }
            }
            PlanHandleState::Existing { plan_id, current } => {
                if let Some(view) = committed_plan_view(commit, plan_id) {
                    *current = view;
                }
            }
        }
    }
    for task_state in state.task_handles.values_mut() {
        match task_state {
            TaskHandleState::Created {
                client_task_id,
                committed_task_id,
                current,
            } => {
                let resolved_task_id = commit
                    .get("taskIdsByClientId")
                    .and_then(|mapping| mapping.get(client_task_id.as_str()))
                    .and_then(Value::as_str)
                    .map(str::to_string);
                if let Some(task_id) = resolved_task_id {
                    *committed_task_id = Some(task_id.clone());
                    if let Some(view) = committed_task_view(commit, &task_id) {
                        *current = view;
                    } else {
                        *current = json!({ "id": task_id });
                    }
                }
            }
            TaskHandleState::Existing { task_id, current } => {
                if let Some(view) = committed_task_view(commit, task_id) {
                    *current = view;
                }
            }
        }
    }
    for claim_state in state.claim_handles.values_mut() {
        if let Some(claim_id) = claim_state
            .client_id
            .as_ref()
            .and_then(|client_id| {
                commit
                    .get("claimIdsByClientId")
                    .and_then(|mapping| mapping.get(client_id.as_str()))
                    .and_then(Value::as_str)
            })
            .map(str::to_string)
        {
            claim_state.client_id = None;
            if let Some(view) = committed_claim_view(commit, &claim_id) {
                claim_state.current = view;
            } else {
                claim_state.current = json!({ "id": claim_id });
            }
            continue;
        }
        if let Some(claim_id) = preview_id(&claim_state.current) {
            if let Some(view) = committed_claim_view(commit, claim_id) {
                claim_state.current = view;
            }
        }
    }
    for artifact_state in state.artifact_handles.values_mut() {
        if let Some(artifact_id) = artifact_state
            .client_id
            .as_ref()
            .and_then(|client_id| {
                commit
                    .get("artifactIdsByClientId")
                    .and_then(|mapping| mapping.get(client_id.as_str()))
                    .and_then(Value::as_str)
            })
            .map(str::to_string)
        {
            artifact_state.client_id = None;
            if let Some(view) = committed_artifact_view(commit, &artifact_id) {
                artifact_state.current = view;
            } else {
                artifact_state.current = json!({ "id": artifact_id });
            }
            continue;
        }
        if let Some(artifact_id) = preview_id(&artifact_state.current) {
            if let Some(view) = committed_artifact_view(commit, artifact_id) {
                artifact_state.current = view;
            }
        }
    }
}

fn apply_session_effect_results(state: &mut PrismCodeWriteState, commit: &Value) {
    let Some(results) = commit
        .get("workResultsByHandleId")
        .and_then(Value::as_object)
    else {
        return;
    };
    for (handle_id, result) in results {
        if let Some(work_state) = state.work_handles.get_mut(handle_id) {
            work_state.client_id = None;
            work_state.current = result.clone();
        }
    }
}

fn reject_coordination_commit_if_needed(commit: &Value) -> Result<()> {
    if let Some(message) = commit
        .get("rejection")
        .and_then(|rejection| rejection.get("message"))
        .and_then(Value::as_str)
    {
        return Err(anyhow!("coordination transaction rejected: {message}"));
    }
    if commit
        .get("outcome")
        .and_then(Value::as_str)
        .is_some_and(|outcome| outcome.eq_ignore_ascii_case("rejected"))
    {
        return Err(anyhow!(
            "coordination transaction rejected without a rejection message"
        ));
    }
    Ok(())
}

fn committed_plan_view(commit: &Value, plan_id: &str) -> Option<Value> {
    commit
        .get("plans")
        .and_then(Value::as_array)
        .and_then(|plans| {
            plans.iter().find(|plan| {
                plan.get("id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| id == plan_id)
            })
        })
        .cloned()
}

fn committed_task_view(commit: &Value, task_id: &str) -> Option<Value> {
    commit
        .get("tasks")
        .and_then(Value::as_array)
        .and_then(|tasks| {
            tasks.iter().find(|task| {
                task.get("id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| id == task_id)
            })
        })
        .cloned()
}

fn committed_claim_view(commit: &Value, claim_id: &str) -> Option<Value> {
    commit
        .get("claims")
        .and_then(Value::as_array)
        .and_then(|claims| {
            claims.iter().find(|claim| {
                claim.get("id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| id == claim_id)
            })
        })
        .cloned()
}

fn committed_artifact_view(commit: &Value, artifact_id: &str) -> Option<Value> {
    commit
        .get("artifacts")
        .and_then(Value::as_array)
        .and_then(|artifacts| {
            artifacts.iter().find(|artifact| {
                artifact
                    .get("id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| id == artifact_id)
            })
        })
        .cloned()
}

fn resolve_handles(value: Value, state: &PrismCodeWriteState) -> Value {
    match value {
        Value::Array(values) => Value::Array(
            values
                .into_iter()
                .map(|entry| resolve_handles(entry, state))
                .collect(),
        ),
        Value::Object(object) => {
            if let Some(resolved) = resolve_handle_object(&object, state) {
                return resolved;
            }
            Value::Object(
                object
                    .into_iter()
                    .map(|(key, entry)| (key, resolve_handles(entry, state)))
                    .collect(),
            )
        }
        other => other,
    }
}

fn resolve_handle_object(
    object: &Map<String, Value>,
    state: &PrismCodeWriteState,
) -> Option<Value> {
    let handle_kind = object.get(HANDLE_KIND_KEY)?.as_str()?;
    let handle_id = object.get(HANDLE_ID_KEY)?.as_str()?;
    match handle_kind {
        PLAN_HANDLE_KIND => resolve_plan_handle(handle_id, state),
        TASK_HANDLE_KIND => resolve_task_handle(handle_id, state),
        CLAIM_HANDLE_KIND => resolve_deferred_handle(handle_id, &state.claim_handles),
        ARTIFACT_HANDLE_KIND => resolve_deferred_handle(handle_id, &state.artifact_handles),
        WORK_HANDLE_KIND => resolve_deferred_handle(handle_id, &state.work_handles),
        _ => None,
    }
}

fn resolve_plan_handle(handle_id: &str, state: &PrismCodeWriteState) -> Option<Value> {
    match state.plan_handles.get(handle_id) {
        Some(
            PlanHandleState::Created { current, .. } | PlanHandleState::Existing { current, .. },
        ) => Some(current.clone()),
        None => None,
    }
}

fn resolve_task_handle(handle_id: &str, state: &PrismCodeWriteState) -> Option<Value> {
    match state.task_handles.get(handle_id) {
        Some(
            TaskHandleState::Created { current, .. } | TaskHandleState::Existing { current, .. },
        ) => Some(current.clone()),
        None => None,
    }
}

fn resolve_deferred_handle(
    handle_id: &str,
    handles: &BTreeMap<String, DeferredHandleState>,
) -> Option<Value> {
    handles.get(handle_id).map(|state| state.current.clone())
}

fn plan_handle_with_preview(handle_id: &str, preview: &Value) -> Value {
    merge_handle_with_preview(preview, PLAN_HANDLE_KIND, handle_id)
}

fn task_handle_with_preview(handle_id: &str, preview: &Value) -> Value {
    merge_handle_with_preview(preview, TASK_HANDLE_KIND, handle_id)
}

fn generic_handle_with_preview(handle_id: &str, handle_kind: &str, preview: &Value) -> Value {
    merge_handle_with_preview(preview, handle_kind, handle_id)
}

fn merge_handle_with_preview(preview: &Value, handle_kind: &str, handle_id: &str) -> Value {
    let mut object = preview.as_object().cloned().unwrap_or_default();
    object.insert(
        HANDLE_KIND_KEY.to_string(),
        Value::String(handle_kind.to_string()),
    );
    object.insert(
        HANDLE_ID_KEY.to_string(),
        Value::String(handle_id.to_string()),
    );
    Value::Object(object)
}

fn ensure_task_handle_from_value_with_state(
    state: &mut PrismCodeWriteState,
    value: Value,
    method: &str,
    allow_provisional: bool,
) -> Result<String> {
    if let Some(handle_id) = handle_id_from_value(&value, TASK_HANDLE_KIND)? {
        if !allow_provisional {
            match state.task_handles.get(&handle_id) {
                Some(TaskHandleState::Created {
                    committed_task_id: None,
                    ..
                }) => {}
                Some(_) => {}
                None => return Err(anyhow!("unknown task handle `{handle_id}`")),
            }
        }
        return Ok(handle_id);
    }
    let task_id = existing_object_id_from_value(&value, method, "task")?;
    let handle_id = format!("task-handle:{}", state.next_task_handle);
    state.next_task_handle += 1;
    let preview = json!({
        "id": task_id,
        "provisional": false,
    });
    state.task_handles.insert(
        handle_id.clone(),
        TaskHandleState::Existing {
            task_id,
            current: preview,
        },
    );
    Ok(handle_id)
}

fn handle_id_from_value(value: &Value, expected_kind: &str) -> Result<Option<String>> {
    let Some(object) = value.as_object() else {
        return Ok(None);
    };
    let Some(handle_kind) = object.get(HANDLE_KIND_KEY).and_then(Value::as_str) else {
        return Ok(None);
    };
    if handle_kind != expected_kind {
        return Err(anyhow!("expected a `{expected_kind}` handle"));
    }
    Ok(object
        .get(HANDLE_ID_KEY)
        .and_then(Value::as_str)
        .map(str::to_string))
}

fn plan_ref_value(state: &PrismCodeWriteState, handle_id: &str) -> Result<Value> {
    match state.plan_handles.get(handle_id) {
        Some(PlanHandleState::Created {
            client_plan_id,
            committed_plan_id,
            ..
        }) => match committed_plan_id {
            Some(plan_id) => Ok(json!({ "planId": plan_id })),
            None => Ok(json!({ "clientPlanId": client_plan_id })),
        },
        Some(PlanHandleState::Existing { plan_id, .. }) => Ok(json!({ "planId": plan_id })),
        None => Err(anyhow!("unknown plan handle `{handle_id}`")),
    }
}

fn task_ref_values(state: &PrismCodeWriteState, handle_ids: &[String]) -> Result<Vec<Value>> {
    handle_ids
        .iter()
        .map(|handle_id| task_ref_value(state, handle_id, "task reference"))
        .collect()
}

fn task_ref_value(state: &PrismCodeWriteState, handle_id: &str, method: &str) -> Result<Value> {
    match state.task_handles.get(handle_id) {
        Some(TaskHandleState::Created {
            client_task_id,
            committed_task_id,
            ..
        }) => match committed_task_id {
            Some(task_id) => Ok(json!({ "taskId": task_id })),
            None => Ok(json!({ "clientTaskId": client_task_id })),
        },
        Some(TaskHandleState::Existing { task_id, .. }) => Ok(json!({ "taskId": task_id })),
        None => Err(anyhow!("unknown task handle `{handle_id}` for `{method}`")),
    }
}

fn claim_ref_value(state: &PrismCodeWriteState, handle_id: &str, method: &str) -> Result<Value> {
    match state.claim_handles.get(handle_id) {
        Some(DeferredHandleState {
            client_id: Some(client_id),
            ..
        }) => Ok(json!({ "clientClaimId": client_id })),
        Some(DeferredHandleState { current, .. }) => preview_id(current)
            .map(|claim_id| json!({ "claimId": claim_id }))
            .ok_or_else(|| anyhow!("`{method}` requires a resolved claim id")),
        None => Err(anyhow!("unknown claim handle `{handle_id}` for `{method}`")),
    }
}

fn artifact_ref_value(
    state: &PrismCodeWriteState,
    handle_id: &str,
    method: &str,
) -> Result<Value> {
    match state.artifact_handles.get(handle_id) {
        Some(DeferredHandleState {
            client_id: Some(client_id),
            ..
        }) => Ok(json!({ "clientArtifactId": client_id })),
        Some(DeferredHandleState { current, .. }) => preview_id(current)
            .map(|artifact_id| json!({ "artifactId": artifact_id }))
            .ok_or_else(|| anyhow!("`{method}` requires a resolved artifact id")),
        None => Err(anyhow!(
            "unknown artifact handle `{handle_id}` for `{method}`"
        )),
    }
}

fn insert_value_if_present(target: &mut Map<String, Value>, key: &str, value: Option<Value>) {
    if let Some(value) = value {
        target.insert(key.to_string(), value);
    }
}

fn insert_string_if_present(target: &mut Map<String, Value>, key: &str, value: Option<String>) {
    if let Some(value) = value {
        target.insert(key.to_string(), Value::String(value));
    }
}

fn set_optional_string_field(target: &mut Map<String, Value>, key: &str, value: Option<String>) {
    if let Some(value) = value {
        target.insert(key.to_string(), Value::String(value));
    }
}

fn set_optional_value_field(target: &mut Map<String, Value>, key: &str, value: Option<Value>) {
    if let Some(value) = value {
        target.insert(key.to_string(), value);
    }
}

fn set_optional_patch_field(target: &mut Map<String, Value>, key: &str, value: Option<Value>) {
    match value {
        Some(Value::Object(object))
            if object.get("op").and_then(Value::as_str) == Some("clear") =>
        {
            target.remove(key);
        }
        Some(value) => {
            target.insert(key.to_string(), value);
        }
        None => {}
    }
}

fn expect_object(value: Value, method: &str) -> Result<Map<String, Value>> {
    value
        .as_object()
        .cloned()
        .ok_or_else(|| anyhow!("`{method}` expects a plain object input"))
}

fn required_string(object: &Map<String, Value>, key: &str, method: &str) -> Result<String> {
    object
        .get(key)
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("`{method}` requires a non-empty `{key}` string"))
}

fn optional_string(object: &Map<String, Value>, key: &str) -> Option<String> {
    object
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn plan_handle_id_from_value(value: &Value, method: &str) -> Result<Option<String>> {
    let Some(object) = value.as_object() else {
        return Ok(None);
    };
    let Some(handle_kind) = object.get(HANDLE_KIND_KEY).and_then(Value::as_str) else {
        return Ok(None);
    };
    if handle_kind != PLAN_HANDLE_KIND {
        return Err(anyhow!("`{method}` expects a plan handle"));
    }
    Ok(object
        .get(HANDLE_ID_KEY)
        .and_then(Value::as_str)
        .map(str::to_string))
}

fn optional_string_patch(
    object: &Map<String, Value>,
    key: &str,
    method: &str,
) -> Result<Option<Value>> {
    let Some(value) = object.get(key) else {
        return Ok(None);
    };
    match value {
        Value::Null => Ok(Some(json!({ "op": "clear" }))),
        Value::String(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return Err(anyhow!(
                    "`{method}` requires `{key}` strings to be non-empty"
                ));
            }
            Ok(Some(Value::String(trimmed.to_string())))
        }
        _ => Err(anyhow!(
            "`{method}` expects `{key}` to be a string or null when provided"
        )),
    }
}

fn optional_u8_patch(
    object: &Map<String, Value>,
    key: &str,
    method: &str,
) -> Result<Option<Value>> {
    let Some(value) = object.get(key) else {
        return Ok(None);
    };
    match value {
        Value::Null => Ok(Some(json!({ "op": "clear" }))),
        Value::Number(number) => {
            let Some(value) = number.as_u64() else {
                return Err(anyhow!(
                    "`{method}` expects `{key}` to be a non-negative integer or null when provided"
                ));
            };
            let value = u8::try_from(value)
                .map_err(|_| anyhow!("`{method}` expects `{key}` to fit in the 0..=255 range"))?;
            Ok(Some(Value::Number(value.into())))
        }
        _ => Err(anyhow!(
            "`{method}` expects `{key}` to be a non-negative integer or null when provided"
        )),
    }
}

fn non_empty_string(value: String, method: &str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("`{method}` requires a non-empty string"));
    }
    Ok(trimmed.to_string())
}

fn existing_object_id_from_value(value: &Value, method: &str, kind: &str) -> Result<String> {
    if let Some(id) = value.as_str() {
        return non_empty_string(id.to_string(), method);
    }
    let Some(object) = value.as_object() else {
        return Err(anyhow!(
            "`{method}` expects a {kind} object or {kind} id string"
        ));
    };
    object
        .get("id")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow!("`{method}` requires a {kind} object with a non-empty `id`"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prism_code_compiler::{
        analyze_prepared_typescript_program, prepare_typescript_program, PrismCodeCompilerInput,
        PrismTypescriptProgramMode,
    };
    use crate::QueryLanguage;
    use serde_json::json;

    fn analyze(code: &str) -> AnalyzedPrismProgram {
        let input = PrismCodeCompilerInput::inline("prism_code", code, QueryLanguage::Ts, true);
        let prepared =
            prepare_typescript_program(&input, None, PrismTypescriptProgramMode::StatementBody)
                .expect("program should prepare");
        analyze_prepared_typescript_program(&prepared).expect("program should analyze")
    }

    fn runtime_for(code: &str) -> PrismCodeWriteRuntime {
        PrismCodeWriteRuntimeFactory::new(Arc::new(|_| Ok(json!({}))), true).instantiate(analyze(code))
    }

    fn handle_id(value: &Value, kind: &str) -> String {
        let object = value.as_object().expect("handle should be an object");
        assert_eq!(
            object
                .get(HANDLE_KIND_KEY)
                .and_then(Value::as_str)
                .expect("handle kind"),
            kind
        );
        object
            .get(HANDLE_ID_KEY)
            .and_then(Value::as_str)
            .expect("handle id")
            .to_string()
    }

    #[test]
    fn write_runtime_preserves_parallel_region_structure() {
        let runtime = runtime_for(
            r#"
const plan = await prism.coordination.createPlan({ title: "Ship" });
await Promise.all([
  plan.addTask({ title: "Build" }),
  plan.addTask({ title: "Test" }),
]);
"#,
        );
        let plan = runtime
            .create_plan(json!({ "title": "Ship" }))
            .expect("plan create should stage");
        let plan_handle_id = handle_id(&plan, PLAN_HANDLE_KIND);
        runtime
            .plan_add_task(plan_handle_id.clone(), json!({ "title": "Build" }))
            .expect("first task should stage");
        runtime
            .plan_add_task(plan_handle_id, json!({ "title": "Test" }))
            .expect("second task should stage");

        let transaction_plan = runtime.debug_transaction_plan();
        let parallel_region = transaction_plan
            .regions
            .iter()
            .find(|region| {
                matches!(
                    region.control,
                    super::super::program_ir::PrismProgramRegionControl::Parallel {
                        kind: super::super::program_ir::PrismProgramParallelKind::PromiseAll,
                        ..
                    }
                )
            })
            .expect("parallel region should be preserved");
        assert_eq!(parallel_region.effect_ids.len(), 2);
        assert_eq!(
            transaction_plan
                .ordered_effect_ids()
                .into_iter()
                .map(|id| transaction_plan.effects[id].metadata.method_path.clone())
                .collect::<Vec<_>>(),
            vec![
                "prism.coordination.createPlan".to_string(),
                "plan.addTask".to_string(),
                "plan.addTask".to_string(),
            ]
        );
    }

    #[test]
    fn write_runtime_preserves_loop_and_finally_structure_and_source_methods() {
        let runtime = runtime_for(
            r#"
const plan = await prism.coordination.createPlan({ title: "Ship" });
const task = await plan.addTask({ title: "Baseline" });
for (const title of ["A", "B"]) {
  await plan.addTask({ title });
}
try {
  await task.complete({ summary: "done" });
} finally {
  await task.handoff({ summary: "handoff" });
}
"#,
        );
        let plan = runtime
            .create_plan(json!({ "title": "Ship" }))
            .expect("plan create should stage");
        let plan_handle_id = handle_id(&plan, PLAN_HANDLE_KIND);
        let task = runtime
            .plan_add_task(plan_handle_id.clone(), json!({ "title": "Baseline" }))
            .expect("baseline task should stage");
        runtime
            .plan_add_task(plan_handle_id.clone(), json!({ "title": "A" }))
            .expect("loop task A should stage");
        runtime
            .plan_add_task(plan_handle_id, json!({ "title": "B" }))
            .expect("loop task B should stage");
        runtime
            .task_complete(task.clone(), json!({ "summary": "done" }))
            .expect("task complete should stage");
        runtime
            .task_handoff(task, json!({ "summary": "handoff" }))
            .expect("task handoff should stage");

        let transaction_plan = runtime.debug_transaction_plan();
        assert!(transaction_plan.regions.iter().any(|region| matches!(
            region.control,
            super::super::program_ir::PrismProgramRegionControl::Loop {
                kind: super::super::program_ir::PrismProgramLoopKind::ForOf,
                ..
            }
        )));
        assert!(transaction_plan.regions.iter().any(|region| {
            matches!(
                region.control,
                super::super::program_ir::PrismProgramRegionControl::TryCatchFinally
            )
        }));
        assert!(transaction_plan.effects.iter().any(|effect| {
            effect.metadata.method_path == "task.complete"
                && matches!(
                    effect.payload,
                    StructuredWriteOperationKind::Coordination(
                        CoordinationWriteOp::TaskUpdate { .. }
                    )
                )
        }));
        assert!(transaction_plan.effects.iter().any(|effect| {
            effect.metadata.method_path == "task.handoff"
                && matches!(
                    effect.payload,
                    StructuredWriteOperationKind::Coordination(
                        CoordinationWriteOp::TaskHandoff { .. }
                    )
                )
        }));
    }

    #[test]
    fn write_runtime_surfaces_coordination_rejections() {
        let analyzed = analyze(
            r#"
const plan = await prism.coordination.createPlan({ title: "Ship" });
return { plan };
"#,
        );
        let runtime = PrismCodeWriteRuntimeFactory::new(
            Arc::new(|_| {
                Ok(json!({
                    "outcome": "Rejected",
                    "rejection": {
                        "message": "simulated coordination rejection",
                    }
                }))
            }),
            false,
        )
        .instantiate(analyzed);

        let plan = runtime
            .create_plan(json!({ "title": "Ship" }))
            .expect("plan create should stage");
        let error = runtime
            .finalize_result(json!({ "plan": plan }))
            .expect_err("rejection should surface");
        assert!(
            error
                .to_string()
                .contains("simulated coordination rejection"),
            "unexpected error: {error}"
        );
    }
}
