use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
use serde_json::{json, Map, Value};

pub(crate) enum PrismCodeDirectWrite {
    DeclareWork { input: Value },
    ClaimAcquire { input: Value },
    ClaimRenew { claim_id: String, ttl_seconds: Option<u64> },
    ClaimRelease { claim_id: String },
    ArtifactPropose { input: Value },
    ArtifactSupersede { artifact_id: String },
    ArtifactReview {
        artifact_id: String,
        input: Value,
    },
    TaskHandoff {
        task_id: String,
        summary: String,
        to_agent: Option<String>,
    },
    TaskAcceptHandoff { task_id: String, agent: Option<String> },
    TaskResume { task_id: String, agent: Option<String> },
    TaskReclaim { task_id: String, agent: Option<String> },
}

type DirectWriteExecutor = Arc<dyn Fn(PrismCodeDirectWrite) -> Result<Value> + Send + Sync>;
type CoordinationCommitExecutor = Arc<dyn Fn(Value) -> Result<Value> + Send + Sync>;

const HANDLE_KIND_KEY: &str = "__prismCoordinationHandleKind";
const HANDLE_ID_KEY: &str = "__prismCoordinationHandleId";
const PLAN_HANDLE_KIND: &str = "plan";
const TASK_HANDLE_KIND: &str = "task";

#[derive(Clone)]
pub(crate) struct PrismCodeExecutionContext {
    direct_write_executor: DirectWriteExecutor,
    coordination_executor: CoordinationCommitExecutor,
    dry_run: bool,
    state: Arc<Mutex<PrismCodeExecutionState>>,
}

#[derive(Default)]
struct PrismCodeExecutionState {
    direct_write_used: bool,
    staged_coordination: Option<StagedCoordinationTransaction>,
}

#[derive(Default)]
struct StagedCoordinationTransaction {
    next_plan_handle: usize,
    next_task_handle: usize,
    next_client_plan: usize,
    next_client_task: usize,
    mutations: Vec<Value>,
    plan_handles: BTreeMap<String, PlanHandleState>,
    task_handles: BTreeMap<String, TaskHandleState>,
}

enum PlanHandleState {
    Created {
        client_plan_id: String,
        preview: Value,
    },
    Existing {
        plan_id: String,
        preview: Value,
    },
}

enum TaskHandleState {
    Created {
        client_task_id: String,
        preview: Value,
    },
    Existing {
        task_id: String,
        preview: Value,
    },
}

impl PrismCodeExecutionContext {
    pub(crate) fn new(
        direct_write_executor: DirectWriteExecutor,
        coordination_executor: CoordinationCommitExecutor,
        dry_run: bool,
    ) -> Self {
        Self {
            direct_write_executor,
            coordination_executor,
            dry_run,
            state: Arc::new(Mutex::new(PrismCodeExecutionState::default())),
        }
    }

    pub(crate) fn declare_work(&self, input: Value) -> Result<Value> {
        self.execute_direct_write(PrismCodeDirectWrite::DeclareWork { input })
    }

    pub(crate) fn claim_acquire(&self, input: Value) -> Result<Value> {
        let mut input = expect_object(input, "prism.claim.acquire")?;
        if let Some(task) = input.get("coordinationTaskId").cloned() {
            let task_id = self.existing_task_id_from_value(&task, "prism.claim.acquire")?;
            input.insert("coordinationTaskId".to_string(), Value::String(task_id));
        }
        self.execute_direct_write(PrismCodeDirectWrite::ClaimAcquire {
            input: Value::Object(input),
        })
    }

    pub(crate) fn claim_renew(&self, claim: Value, input: Value) -> Result<Value> {
        let input = expect_object(input, "prism.claim.renew")?;
        let claim_id = self.existing_claim_id_from_value(&claim, "prism.claim.renew")?;
        self.execute_direct_write(PrismCodeDirectWrite::ClaimRenew {
            claim_id,
            ttl_seconds: input.get("ttlSeconds").and_then(Value::as_u64),
        })
    }

    pub(crate) fn claim_release(&self, claim: Value) -> Result<Value> {
        let claim_id = self.existing_claim_id_from_value(&claim, "prism.claim.release")?;
        self.execute_direct_write(PrismCodeDirectWrite::ClaimRelease { claim_id })
    }

    pub(crate) fn artifact_propose(&self, input: Value) -> Result<Value> {
        let mut input = expect_object(input, "prism.artifact.propose")?;
        if let Some(task) = input.get("taskId").cloned() {
            let task_id = self.existing_task_id_from_value(&task, "prism.artifact.propose")?;
            input.insert("taskId".to_string(), Value::String(task_id));
        }
        self.execute_direct_write(PrismCodeDirectWrite::ArtifactPropose {
            input: Value::Object(input),
        })
    }

    pub(crate) fn artifact_supersede(&self, artifact: Value) -> Result<Value> {
        let artifact_id =
            self.existing_artifact_id_from_value(&artifact, "prism.artifact.supersede")?;
        self.execute_direct_write(PrismCodeDirectWrite::ArtifactSupersede { artifact_id })
    }

    pub(crate) fn artifact_review(&self, artifact: Value, input: Value) -> Result<Value> {
        let input = expect_object(input, "prism.artifact.review")?;
        let artifact_id =
            self.existing_artifact_id_from_value(&artifact, "prism.artifact.review")?;
        self.execute_direct_write(PrismCodeDirectWrite::ArtifactReview {
            artifact_id,
            input: Value::Object(input),
        })
    }

    pub(crate) fn task_handoff(&self, task: Value, input: Value) -> Result<Value> {
        let input = expect_object(input, "task.handoff")?;
        let task_id = self.existing_task_id_from_value(&task, "task.handoff")?;
        let summary = required_string(&input, "summary", "task.handoff")?;
        let to_agent =
            optional_string(&input, "toAgent").or_else(|| optional_string(&input, "to_agent"));
        self.execute_direct_write(PrismCodeDirectWrite::TaskHandoff {
            task_id,
            summary,
            to_agent,
        })
    }

    pub(crate) fn task_accept_handoff(&self, task: Value, input: Value) -> Result<Value> {
        let input = expect_object(input, "task.acceptHandoff")?;
        let task_id = self.existing_task_id_from_value(&task, "task.acceptHandoff")?;
        let agent = optional_string(&input, "agent");
        self.execute_direct_write(PrismCodeDirectWrite::TaskAcceptHandoff { task_id, agent })
    }

    pub(crate) fn task_resume(&self, task: Value, input: Value) -> Result<Value> {
        let input = expect_object(input, "task.resume")?;
        let task_id = self.existing_task_id_from_value(&task, "task.resume")?;
        let agent = optional_string(&input, "agent");
        self.execute_direct_write(PrismCodeDirectWrite::TaskResume { task_id, agent })
    }

    pub(crate) fn task_reclaim(&self, task: Value, input: Value) -> Result<Value> {
        let input = expect_object(input, "task.reclaim")?;
        let task_id = self.existing_task_id_from_value(&task, "task.reclaim")?;
        let agent = optional_string(&input, "agent");
        self.execute_direct_write(PrismCodeDirectWrite::TaskReclaim { task_id, agent })
    }

    fn execute_direct_write(&self, input: PrismCodeDirectWrite) -> Result<Value> {
        let mut state = self.state.lock().expect("code mutation lock poisoned");
        if state
            .staged_coordination
            .as_ref()
            .is_some_and(|staged| !staged.mutations.is_empty())
        {
            return Err(anyhow!(
                "native staged coordination builders cannot be mixed with direct write operations in one prism_code invocation"
            ));
        }
        if state.direct_write_used {
            return Err(anyhow!(
                "prism_code currently supports at most one direct write operation per invocation outside the staged coordination builder path"
            ));
        }
        state.direct_write_used = true;
        drop(state);
        (self.direct_write_executor)(input)
    }

    pub(crate) fn create_plan(&self, input: Value) -> Result<Value> {
        let mut state = self.state.lock().expect("code mutation lock poisoned");
        let staged = state.staged_coordination_mut()?;
        let input = expect_object(input, "prism.coordination.createPlan")?;
        let title = required_string(&input, "title", "prism.coordination.createPlan")?;
        let goal = optional_string(&input, "goal").unwrap_or_else(|| title.clone());
        let status = optional_string(&input, "status");
        let policy = input.get("policy").cloned();
        let scheduling = input.get("scheduling").cloned();
        let handle_id = format!("plan-handle:{}", staged.next_plan_handle);
        staged.next_plan_handle += 1;
        let client_plan_id = format!("plan_{}", staged.next_client_plan);
        staged.next_client_plan += 1;
        staged.mutations.push(json!({
            "action": "plan_create",
            "input": {
                "clientPlanId": client_plan_id,
                "title": title,
                "goal": goal,
                "status": status,
                "policy": policy,
                "scheduling": scheduling,
            }
        }));
        let preview = json!({
            "id": client_plan_id,
            "title": input.get("title").cloned().unwrap_or(Value::Null),
            "goal": goal,
            "status": status.clone().unwrap_or_else(|| "draft".to_string()),
            "provisional": true,
        });
        staged.plan_handles.insert(
            handle_id.clone(),
            PlanHandleState::Created {
                client_plan_id,
                preview,
            },
        );
        Ok(plan_handle_with_preview(
            &handle_id,
            staged
                .plan_handles
                .get(&handle_id)
                .and_then(plan_state_preview)
                .expect("created plan preview should exist"),
        ))
    }

    pub(crate) fn open_plan(&self, plan_id: String) -> Result<Value> {
        let mut state = self.state.lock().expect("code mutation lock poisoned");
        let staged = state.staged_coordination_mut()?;
        let plan_id = non_empty_string(plan_id, "prism.coordination.openPlan")?;
        let handle_id = format!("plan-handle:{}", staged.next_plan_handle);
        staged.next_plan_handle += 1;
        staged.plan_handles.insert(
            handle_id.clone(),
            PlanHandleState::Existing {
                preview: json!({
                    "id": plan_id,
                    "provisional": false,
                }),
                plan_id,
            },
        );
        Ok(plan_handle_with_preview(
            &handle_id,
            staged
                .plan_handles
                .get(&handle_id)
                .and_then(plan_state_preview)
                .expect("existing plan preview should exist"),
        ))
    }

    pub(crate) fn plan_update(&self, plan: Value, input: Value) -> Result<Value> {
        let mut state = self.state.lock().expect("code mutation lock poisoned");
        let staged = state.staged_coordination_mut()?;
        let handle_id = plan_handle_id_from_value(&plan, "plan.update")?
            .ok_or_else(|| anyhow!("`plan.update` requires a plan handle"))?;
        let plan = staged.plan_ref_value(&handle_id)?;
        let input = expect_object(input, "plan.update")?;
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
        staged.mutations.push(json!({
            "action": "plan_update",
            "input": {
                "plan": plan,
                "title": title,
                "goal": goal,
                "status": status,
                "policy": policy,
                "scheduling": scheduling,
            }
        }));
        Ok(plan_handle_with_preview(
            &handle_id,
            staged
                .plan_handles
                .get(&handle_id)
                .and_then(plan_state_preview)
                .expect("updated plan preview should exist"),
        ))
    }

    pub(crate) fn plan_archive(&self, plan: Value) -> Result<Value> {
        let mut state = self.state.lock().expect("code mutation lock poisoned");
        let staged = state.staged_coordination_mut()?;
        let handle_id = plan_handle_id_from_value(&plan, "plan.archive")?
            .ok_or_else(|| anyhow!("`plan.archive` requires a plan handle"))?;
        let plan = staged.plan_ref_value(&handle_id)?;
        staged.mutations.push(json!({
            "action": "plan_archive",
            "input": {
                "plan": plan,
            }
        }));
        Ok(plan_handle_with_preview(
            &handle_id,
            staged
                .plan_handles
                .get(&handle_id)
                .and_then(plan_state_preview)
                .expect("archived plan preview should exist"),
        ))
    }

    pub(crate) fn open_task(&self, task_id: String) -> Result<Value> {
        let mut state = self.state.lock().expect("code mutation lock poisoned");
        let staged = state.staged_coordination_mut()?;
        let task_id = non_empty_string(task_id, "prism.coordination.openTask")?;
        let handle_id = format!("task-handle:{}", staged.next_task_handle);
        staged.next_task_handle += 1;
        staged.task_handles.insert(
            handle_id.clone(),
            TaskHandleState::Existing {
                preview: json!({
                    "id": task_id,
                    "provisional": false,
                }),
                task_id,
            },
        );
        Ok(task_handle_with_preview(
            &handle_id,
            staged
                .task_handles
                .get(&handle_id)
                .and_then(task_state_preview)
                .expect("existing task preview should exist"),
        ))
    }

    pub(crate) fn plan_add_task(&self, plan_handle_id: String, input: Value) -> Result<Value> {
        let mut state = self.state.lock().expect("code mutation lock poisoned");
        let staged = state.staged_coordination_mut()?;
        let plan_ref = staged.plan_ref_value(&plan_handle_id)?;
        let input = expect_object(input, "plan.addTask")?;
        let title = required_string(&input, "title", "plan.addTask")?;
        let status = optional_string(&input, "status");
        let depends_on = input
            .get("dependsOn")
            .or_else(|| input.get("depends_on"))
            .map(|value| staged.task_ref_list(value, "plan.addTask"))
            .transpose()?
            .unwrap_or_default();
        let handle_id = format!("task-handle:{}", staged.next_task_handle);
        staged.next_task_handle += 1;
        let client_task_id = format!("task_{}", staged.next_client_task);
        staged.next_client_task += 1;
        let mut task_input = Map::new();
        task_input.insert(
            "clientTaskId".to_string(),
            Value::String(client_task_id.clone()),
        );
        task_input.insert("plan".to_string(), plan_ref);
        task_input.insert("title".to_string(), Value::String(title));
        if let Some(status) = status.clone() {
            task_input.insert("status".to_string(), Value::String(status));
        }
        if !depends_on.is_empty() {
            task_input.insert("dependsOn".to_string(), Value::Array(depends_on));
        }
        insert_object_field_if_present(&mut task_input, &input, "assignee");
        insert_object_field_if_present(&mut task_input, &input, "anchors");
        insert_object_field_if_present(&mut task_input, &input, "acceptance");
        insert_object_field_if_present(&mut task_input, &input, "artifactRequirements");
        insert_object_field_if_present(&mut task_input, &input, "reviewRequirements");
        staged.mutations.push(json!({
            "action": "task_create",
            "input": task_input,
        }));
        let preview = json!({
            "id": client_task_id,
            "title": input.get("title").cloned().unwrap_or(Value::Null),
            "status": status.clone().unwrap_or_else(|| "proposed".to_string()),
            "provisional": true,
        });
        staged.task_handles.insert(
            handle_id.clone(),
            TaskHandleState::Created {
                client_task_id,
                preview,
            },
        );
        Ok(task_handle_with_preview(
            &handle_id,
            staged
                .task_handles
                .get(&handle_id)
                .and_then(task_state_preview)
                .expect("created task preview should exist"),
        ))
    }

    pub(crate) fn task_update(&self, task: Value, input: Value) -> Result<Value> {
        let mut state = self.state.lock().expect("code mutation lock poisoned");
        let staged = state.staged_coordination_mut()?;
        let handle_id = task_handle_id_from_value(&task, "task.update")?
            .ok_or_else(|| anyhow!("`task.update` requires a task handle"))?;
        let task = staged.task_ref_value(&task, "task.update")?;
        let input = expect_object(input, "task.update")?;
        let status = optional_string(&input, "status");
        let title = optional_string(&input, "title");
        let summary = optional_string_patch(&input, "summary", "task.update")?;
        let assignee = optional_string_patch(&input, "assignee", "task.update")?;
        let priority = optional_u8_patch(&input, "priority", "task.update")?;
        let depends_on = input
            .get("dependsOn")
            .or_else(|| input.get("depends_on"))
            .map(|value| staged.task_ref_list(value, "task.update"))
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
        let mut task_input = Map::new();
        task_input.insert("task".to_string(), task);
        if let Some(status) = status {
            task_input.insert("status".to_string(), Value::String(status));
        }
        if let Some(title) = title {
            task_input.insert("title".to_string(), Value::String(title));
        }
        if let Some(summary) = summary {
            task_input.insert("summary".to_string(), summary);
        }
        if let Some(assignee) = assignee {
            task_input.insert("assignee".to_string(), assignee);
        }
        if let Some(priority) = priority {
            task_input.insert("priority".to_string(), priority);
        }
        if let Some(depends_on) = depends_on {
            task_input.insert("dependsOn".to_string(), Value::Array(depends_on));
        }
        insert_object_field_if_present(&mut task_input, &input, "anchors");
        insert_object_field_if_present(&mut task_input, &input, "acceptance");
        insert_object_field_if_present(&mut task_input, &input, "validationRefs");
        insert_object_field_if_present(&mut task_input, &input, "tags");
        insert_object_field_if_present(&mut task_input, &input, "artifactRequirements");
        insert_object_field_if_present(&mut task_input, &input, "reviewRequirements");
        staged.mutations.push(json!({
            "action": "task_update",
            "input": task_input,
        }));
        Ok(task_handle_with_preview(
            &handle_id,
            staged
                .task_handles
                .get(&handle_id)
                .and_then(task_state_preview)
                .expect("updated task preview should exist"),
        ))
    }

    pub(crate) fn task_depends_on(
        &self,
        task: Value,
        depends_on: Value,
        kind: Option<String>,
    ) -> Result<Value> {
        let mut state = self.state.lock().expect("code mutation lock poisoned");
        let staged = state.staged_coordination_mut()?;
        let task = staged.task_ref_value(&task, "task.dependsOn")?;
        let depends_on = staged.task_ref_value(&depends_on, "task.dependsOn")?;
        staged.mutations.push(json!({
            "action": "dependency_create",
            "input": {
                "task": task,
                "dependsOn": depends_on,
                "kind": kind.unwrap_or_else(|| "depends_on".to_string()),
            }
        }));
        Ok(Value::Null)
    }

    pub(crate) fn task_complete(&self, task: Value, input: Value) -> Result<Value> {
        let mut update = expect_object(input, "task.complete")?;
        update.insert("status".to_string(), Value::String("completed".to_string()));
        self.task_update(task, Value::Object(update))
    }

    pub(crate) fn finalize_result(&self, result: Value) -> Result<Value> {
        let mut state = self.state.lock().expect("code mutation lock poisoned");
        if state.direct_write_used {
            return Ok(result);
        }
        let Some(staged) = state.staged_coordination.take() else {
            return Ok(result);
        };
        if staged.mutations.is_empty() {
            return Ok(result);
        }
        if self.dry_run {
            return Ok(resolve_handles(result, &staged, None));
        }
        let commit = (self.coordination_executor)(json!({
            "mutations": staged.mutations,
        }))?;
        Ok(resolve_handles(result, &staged, Some(&commit)))
    }
}

impl PrismCodeExecutionState {
    fn staged_coordination_mut(&mut self) -> Result<&mut StagedCoordinationTransaction> {
        if self.direct_write_used {
            return Err(anyhow!(
                "native staged coordination builders cannot be mixed with direct write operations in one prism_code invocation"
            ));
        }
        Ok(self
            .staged_coordination
            .get_or_insert_with(Default::default))
    }
}

impl StagedCoordinationTransaction {
    fn plan_ref_value(&self, handle_id: &str) -> Result<Value> {
        match self.plan_handles.get(handle_id) {
            Some(PlanHandleState::Created { client_plan_id, .. }) => {
                Ok(json!({ "clientPlanId": client_plan_id }))
            }
            Some(PlanHandleState::Existing { plan_id, .. }) => Ok(json!({ "planId": plan_id })),
            None => Err(anyhow!("unknown plan handle `{handle_id}`")),
        }
    }

    fn task_ref_list(&self, value: &Value, method: &str) -> Result<Vec<Value>> {
        let values = value
            .as_array()
            .ok_or_else(|| anyhow!("`{method}` expects `dependsOn` to be an array"))?;
        values
            .iter()
            .map(|entry| self.task_ref_value(entry, method))
            .collect()
    }

    fn task_ref_value(&self, value: &Value, method: &str) -> Result<Value> {
        if let Some(task_id) = value.as_str() {
            return Ok(json!({ "taskId": task_id }));
        }
        let handle = value.as_object().ok_or_else(|| {
            anyhow!("`{method}` expects task references to be task handles or task id strings")
        })?;
        let handle_kind = handle
            .get(HANDLE_KIND_KEY)
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("`{method}` received an invalid task handle"))?;
        if handle_kind != TASK_HANDLE_KIND {
            return Err(anyhow!(
                "`{method}` expects task handles, not `{handle_kind}` handles"
            ));
        }
        let handle_id = handle
            .get(HANDLE_ID_KEY)
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("`{method}` received a task handle without an internal id"))?;
        match self.task_handles.get(handle_id) {
            Some(TaskHandleState::Created { client_task_id, .. }) => {
                Ok(json!({ "clientTaskId": client_task_id }))
            }
            Some(TaskHandleState::Existing { task_id, .. }) => Ok(json!({ "taskId": task_id })),
            None => Err(anyhow!("unknown task handle `{handle_id}`")),
        }
    }
}

impl PrismCodeExecutionContext {
    fn existing_claim_id_from_value(&self, value: &Value, method: &str) -> Result<String> {
        existing_object_id_from_value(value, method, "claim")
    }

    fn existing_artifact_id_from_value(&self, value: &Value, method: &str) -> Result<String> {
        existing_object_id_from_value(value, method, "artifact")
    }

    fn existing_task_id_from_value(&self, value: &Value, method: &str) -> Result<String> {
        let state = self.state.lock().expect("code mutation lock poisoned");
        if let Some(task_id) = value.as_str() {
            return Ok(non_empty_string(task_id.to_string(), method)?);
        }
        let Some(object) = value.as_object() else {
            return Err(anyhow!(
                "`{method}` expects an existing task handle, task view, or task id string"
            ));
        };
        if let Some(handle_kind) = object.get(HANDLE_KIND_KEY).and_then(Value::as_str) {
            if handle_kind != TASK_HANDLE_KIND {
                return Err(anyhow!(
                    "`{method}` expects a task handle, not `{handle_kind}`"
                ));
            }
            let handle_id = object
                .get(HANDLE_ID_KEY)
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    anyhow!("`{method}` received a task handle without an internal id")
                })?;
            let Some(staged) = state.staged_coordination.as_ref() else {
                return Err(anyhow!("unknown task handle `{handle_id}`"));
            };
            return match staged.task_handles.get(handle_id) {
                Some(TaskHandleState::Existing { task_id, .. }) => Ok(task_id.clone()),
                Some(TaskHandleState::Created { .. }) => Err(anyhow!(
                    "`{method}` requires an existing committed task handle; provisional task handles can only be used with staged native coordination builders"
                )),
                None => Err(anyhow!("unknown task handle `{handle_id}`")),
            };
        }
        object
            .get("id")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                anyhow!("`{method}` requires a task handle or a task object with a non-empty `id`")
            })
    }
}

fn resolve_handles(
    value: Value,
    staged: &StagedCoordinationTransaction,
    commit: Option<&Value>,
) -> Value {
    match value {
        Value::Array(values) => Value::Array(
            values
                .into_iter()
                .map(|entry| resolve_handles(entry, staged, commit))
                .collect(),
        ),
        Value::Object(object) => {
            if let Some(resolved) = resolve_handle_object(&object, staged, commit) {
                return resolved;
            }
            Value::Object(
                object
                    .into_iter()
                    .map(|(key, entry)| (key, resolve_handles(entry, staged, commit)))
                    .collect(),
            )
        }
        other => other,
    }
}

fn resolve_handle_object(
    object: &Map<String, Value>,
    staged: &StagedCoordinationTransaction,
    commit: Option<&Value>,
) -> Option<Value> {
    let handle_kind = object.get(HANDLE_KIND_KEY)?.as_str()?;
    let handle_id = object.get(HANDLE_ID_KEY)?.as_str()?;
    match handle_kind {
        PLAN_HANDLE_KIND => Some(resolve_plan_handle(handle_id, staged, commit)),
        TASK_HANDLE_KIND => Some(resolve_task_handle(handle_id, staged, commit)),
        _ => None,
    }
}

fn resolve_plan_handle(
    handle_id: &str,
    staged: &StagedCoordinationTransaction,
    commit: Option<&Value>,
) -> Value {
    let Some(state) = staged.plan_handles.get(handle_id) else {
        return Value::Null;
    };
    if let Some(commit) = commit {
        let committed_plan_id = match state {
            PlanHandleState::Created { client_plan_id, .. } => commit
                .get("planIdsByClientId")
                .and_then(|mapping| mapping.get(client_plan_id))
                .and_then(Value::as_str)
                .map(str::to_string),
            PlanHandleState::Existing { plan_id, .. } => Some(plan_id.clone()),
        };
        if let Some(plan_id) = committed_plan_id {
            if let Some(view) = commit
                .get("plans")
                .and_then(Value::as_array)
                .and_then(|plans| {
                    plans.iter().find(|plan| {
                        plan.get("id")
                            .and_then(Value::as_str)
                            .is_some_and(|id| id == plan_id)
                    })
                })
            {
                return view.clone();
            }
            return json!({ "id": plan_id });
        }
    }
    match state {
        PlanHandleState::Created { preview, .. } | PlanHandleState::Existing { preview, .. } => {
            preview.clone()
        }
    }
}

fn resolve_task_handle(
    handle_id: &str,
    staged: &StagedCoordinationTransaction,
    commit: Option<&Value>,
) -> Value {
    let Some(state) = staged.task_handles.get(handle_id) else {
        return Value::Null;
    };
    if let Some(commit) = commit {
        let committed_task_id = match state {
            TaskHandleState::Created { client_task_id, .. } => commit
                .get("taskIdsByClientId")
                .and_then(|mapping| mapping.get(client_task_id))
                .and_then(Value::as_str)
                .map(str::to_string),
            TaskHandleState::Existing { task_id, .. } => Some(task_id.clone()),
        };
        if let Some(task_id) = committed_task_id {
            if let Some(view) = commit
                .get("tasks")
                .and_then(Value::as_array)
                .and_then(|tasks| {
                    tasks.iter().find(|task| {
                        task.get("id")
                            .and_then(Value::as_str)
                            .is_some_and(|id| id == task_id)
                    })
                })
            {
                return view.clone();
            }
            return json!({ "id": task_id });
        }
    }
    match state {
        TaskHandleState::Created { preview, .. } | TaskHandleState::Existing { preview, .. } => {
            preview.clone()
        }
    }
}

fn plan_handle_with_preview(handle_id: &str, preview: &Value) -> Value {
    merge_handle_with_preview(preview, PLAN_HANDLE_KIND, handle_id)
}

fn task_handle_with_preview(handle_id: &str, preview: &Value) -> Value {
    merge_handle_with_preview(preview, TASK_HANDLE_KIND, handle_id)
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

fn plan_state_preview(state: &PlanHandleState) -> Option<&Value> {
    match state {
        PlanHandleState::Created { preview, .. } | PlanHandleState::Existing { preview, .. } => {
            Some(preview)
        }
    }
}

fn task_state_preview(state: &TaskHandleState) -> Option<&Value> {
    match state {
        TaskHandleState::Created { preview, .. } | TaskHandleState::Existing { preview, .. } => {
            Some(preview)
        }
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

fn insert_object_field_if_present(
    target: &mut Map<String, Value>,
    source: &Map<String, Value>,
    key: &str,
) {
    if let Some(value) = source.get(key) {
        target.insert(key.to_string(), value.clone());
    }
}

fn task_handle_id_from_value(value: &Value, method: &str) -> Result<Option<String>> {
    let Some(object) = value.as_object() else {
        return Ok(None);
    };
    let Some(handle_kind) = object.get(HANDLE_KIND_KEY).and_then(Value::as_str) else {
        return Ok(None);
    };
    if handle_kind != TASK_HANDLE_KIND {
        return Err(anyhow!("`{method}` expects a task handle"));
    }
    Ok(object
        .get(HANDLE_ID_KEY)
        .and_then(Value::as_str)
        .map(str::to_string))
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
