use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
use serde_json::{json, Map, Value};

use crate::PrismMutationResult;

type LegacyMutationExecutor = Arc<dyn Fn(Value) -> Result<Value> + Send + Sync>;
type CoordinationCommitExecutor = Arc<dyn Fn(Value) -> Result<PrismMutationResult> + Send + Sync>;

const HANDLE_KIND_KEY: &str = "__prismCoordinationHandleKind";
const HANDLE_ID_KEY: &str = "__prismCoordinationHandleId";
const PLAN_HANDLE_KIND: &str = "plan";
const TASK_HANDLE_KIND: &str = "task";

#[derive(Clone)]
pub(crate) struct PrismCodeExecutionContext {
    legacy_executor: LegacyMutationExecutor,
    coordination_executor: CoordinationCommitExecutor,
    dry_run: bool,
    state: Arc<Mutex<PrismCodeExecutionState>>,
}

#[derive(Default)]
struct PrismCodeExecutionState {
    legacy_mutation_used: bool,
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
        legacy_executor: LegacyMutationExecutor,
        coordination_executor: CoordinationCommitExecutor,
        dry_run: bool,
    ) -> Self {
        Self {
            legacy_executor,
            coordination_executor,
            dry_run,
            state: Arc::new(Mutex::new(PrismCodeExecutionState::default())),
        }
    }

    pub(crate) fn execute_legacy_mutation(&self, input: Value) -> Result<Value> {
        let mut state = self.state.lock().expect("code mutation lock poisoned");
        if state
            .staged_coordination
            .as_ref()
            .is_some_and(|staged| !staged.mutations.is_empty())
        {
            return Err(anyhow!(
                "native coordination builders cannot be mixed with `prism.mutate(...)` in one prism_code invocation"
            ));
        }
        if state.legacy_mutation_used {
            return Err(anyhow!(
                "prism_code currently supports at most one `prism.mutate(...)` call per invocation"
            ));
        }
        state.legacy_mutation_used = true;
        drop(state);
        (self.legacy_executor)(input)
    }

    pub(crate) fn create_plan(&self, input: Value) -> Result<Value> {
        let mut state = self.state.lock().expect("code mutation lock poisoned");
        let staged = state.staged_coordination_mut()?;
        let input = expect_object(input, "prism.coordination.createPlan")?;
        let title = required_string(&input, "title", "prism.coordination.createPlan")?;
        let goal = optional_string(&input, "goal").unwrap_or_else(|| title.clone());
        let status = optional_string(&input, "status");
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
        Ok(plan_handle(&handle_id))
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
        Ok(plan_handle(&handle_id))
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
        if title.is_none() && goal.is_none() && status.is_none() {
            return Err(anyhow!(
                "`plan.update` requires at least one of `title`, `goal`, or `status`"
            ));
        }
        staged.mutations.push(json!({
            "action": "plan_update",
            "input": {
                "plan": plan,
                "title": title,
                "goal": goal,
                "status": status,
            }
        }));
        Ok(plan_handle(&handle_id))
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
        Ok(plan_handle(&handle_id))
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
        Ok(task_handle(&handle_id))
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
        task_input.insert("clientTaskId".to_string(), Value::String(client_task_id.clone()));
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
        Ok(task_handle(&handle_id))
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
        Ok(task_handle(&handle_id))
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
        if state.legacy_mutation_used {
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
        if self.legacy_mutation_used {
            return Err(anyhow!(
                "native coordination builders cannot be mixed with `prism.mutate(...)` in one prism_code invocation"
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

fn resolve_handles(
    value: Value,
    staged: &StagedCoordinationTransaction,
    commit: Option<&PrismMutationResult>,
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
    commit: Option<&PrismMutationResult>,
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
    commit: Option<&PrismMutationResult>,
) -> Value {
    let Some(state) = staged.plan_handles.get(handle_id) else {
        return Value::Null;
    };
    if let Some(commit) = commit {
        let committed_plan_id = match state {
            PlanHandleState::Created { client_plan_id, .. } => commit
                .result
                .get("state")
                .and_then(|state| state.get("planIdsByClientId"))
                .and_then(|mapping| mapping.get(client_plan_id))
                .and_then(Value::as_str)
                .map(str::to_string),
            PlanHandleState::Existing { plan_id, .. } => Some(plan_id.clone()),
        };
        if let Some(plan_id) = committed_plan_id {
            if let Some(view) = commit
                .result
                .get("state")
                .and_then(|state| state.get("plans"))
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
    commit: Option<&PrismMutationResult>,
) -> Value {
    let Some(state) = staged.task_handles.get(handle_id) else {
        return Value::Null;
    };
    if let Some(commit) = commit {
        let committed_task_id = match state {
            TaskHandleState::Created { client_task_id, .. } => commit
                .result
                .get("state")
                .and_then(|state| state.get("taskIdsByClientId"))
                .and_then(|mapping| mapping.get(client_task_id))
                .and_then(Value::as_str)
                .map(str::to_string),
            TaskHandleState::Existing { task_id, .. } => Some(task_id.clone()),
        };
        if let Some(task_id) = committed_task_id {
            if let Some(view) = commit
                .result
                .get("state")
                .and_then(|state| state.get("tasks"))
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

fn plan_handle(handle_id: &str) -> Value {
    json!({
        HANDLE_KIND_KEY: PLAN_HANDLE_KIND,
        HANDLE_ID_KEY: handle_id,
    })
}

fn task_handle(handle_id: &str) -> Value {
    json!({
        HANDLE_KIND_KEY: TASK_HANDLE_KIND,
        HANDLE_ID_KEY: handle_id,
    })
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

fn optional_u8_patch(object: &Map<String, Value>, key: &str, method: &str) -> Result<Option<Value>> {
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
            let value = u8::try_from(value).map_err(|_| {
                anyhow!("`{method}` expects `{key}` to fit in the 0..=255 range")
            })?;
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
        return Err(anyhow!("`{method}` requires a non-empty plan id"));
    }
    Ok(trimmed.to_string())
}
