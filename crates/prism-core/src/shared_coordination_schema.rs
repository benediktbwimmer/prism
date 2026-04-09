use anyhow::{anyhow, Context, Result};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};

pub(crate) const SHARED_COORDINATION_SCHEMA_VERSION: u32 = 1;
pub(crate) const SHARED_COORDINATION_KIND_MANIFEST: &str = "coordination_manifest";
pub(crate) const SHARED_COORDINATION_KIND_PLAN_RECORD: &str = "coordination_plan_record";
pub(crate) const SHARED_COORDINATION_KIND_TASK: &str = "coordination_task";
pub(crate) const SHARED_COORDINATION_KIND_ARTIFACT: &str = "coordination_artifact";
pub(crate) const SHARED_COORDINATION_KIND_CLAIM: &str = "coordination_claim";
pub(crate) const SHARED_COORDINATION_KIND_REVIEW: &str = "coordination_review";
pub(crate) const SHARED_COORDINATION_KIND_RUNTIME_DESCRIPTOR: &str = "runtime_descriptor";

pub(crate) fn wrap_authoritative_payload<T>(payload: &T, kind: &str) -> Result<Value>
where
    T: Serialize,
{
    let payload = serde_json::to_value(payload)
        .with_context(|| format!("failed to encode shared coordination payload `{kind}`"))?;
    Ok(json!({
        "schema_version": SHARED_COORDINATION_SCHEMA_VERSION,
        "kind": kind,
        "payload": payload,
    }))
}

pub(crate) fn parse_authoritative_payload<T>(
    bytes: &[u8],
    path: &str,
    expected_kind: &str,
) -> Result<T>
where
    T: DeserializeOwned,
{
    let value: Value = serde_json::from_slice(bytes)
        .with_context(|| format!("failed to parse shared coordination ref file `{path}`"))?;
    let object = value
        .as_object()
        .ok_or_else(|| anyhow!("shared coordination payload `{path}` must be a JSON object"))?;
    let has_envelope = validate_authoritative_envelope(object, path, expected_kind)?;
    if !has_envelope {
        return serde_json::from_value(value)
            .with_context(|| format!("failed to decode shared coordination payload `{path}`"));
    }
    let payload = object
        .get("payload")
        .ok_or_else(|| anyhow!("shared coordination payload `{path}` is missing `payload`"))?;
    serde_json::from_value(payload.clone())
        .with_context(|| format!("failed to decode shared coordination payload `{path}`"))
}

pub(crate) fn parse_top_level_authoritative_payload<T>(
    bytes: &[u8],
    path: &str,
    expected_kind: &str,
) -> Result<T>
where
    T: DeserializeOwned,
{
    let value: Value = serde_json::from_slice(bytes)
        .with_context(|| format!("failed to parse shared coordination ref file `{path}`"))?;
    let object = value
        .as_object()
        .ok_or_else(|| anyhow!("shared coordination payload `{path}` must be a JSON object"))?;
    validate_authoritative_envelope(object, path, expected_kind)?;
    serde_json::from_value(value)
        .with_context(|| format!("failed to decode shared coordination payload `{path}`"))
}

fn validate_authoritative_envelope(
    object: &serde_json::Map<String, Value>,
    path: &str,
    expected_kind: &str,
) -> Result<bool> {
    let has_envelope = object.contains_key("schema_version")
        || object.contains_key("schemaVersion")
        || object.contains_key("version")
        || object.contains_key("payload");
    if !has_envelope {
        return Ok(false);
    }
    let observed_schema_version = object
        .get("schema_version")
        .or_else(|| object.get("schemaVersion"))
        .or_else(|| object.get("version"))
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            anyhow!("shared coordination payload `{path}` is missing a valid `schema_version`")
        })?;
    if observed_schema_version > SHARED_COORDINATION_SCHEMA_VERSION as u64 {
        return Err(anyhow!(
            "shared coordination payload `{path}` requires schema_version {} for kind `{expected_kind}`, but this PRISM supports up to {}. Upgrade PRISM and retry.",
            observed_schema_version,
            SHARED_COORDINATION_SCHEMA_VERSION,
        ));
    }
    if let Some(observed_kind) = object.get("kind").and_then(Value::as_str) {
        if observed_kind != expected_kind {
            return Err(anyhow!(
                "shared coordination payload `{path}` declared kind `{observed_kind}`, expected `{expected_kind}`"
            ));
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::{
        parse_authoritative_payload, wrap_authoritative_payload, SHARED_COORDINATION_KIND_TASK,
    };
    use prism_coordination::{CoordinationTask, TaskGitExecution};
    use prism_ir::{CoordinationTaskId, PlanId, PlanNodeKind, WorkspaceRevision};
    use serde_json::json;

    fn sample_task() -> CoordinationTask {
        CoordinationTask {
            id: CoordinationTaskId::new("coord-task:sample"),
            plan: PlanId::new("plan:sample"),
            kind: PlanNodeKind::Edit,
            title: "Sample".into(),
            summary: None,
            status: prism_ir::CoordinationTaskStatus::Ready,
            published_task_status: None,
            assignee: None,
            pending_handoff_to: None,
            session: None,
            lease_holder: None,
            lease_started_at: None,
            lease_refreshed_at: None,
            lease_stale_at: None,
            lease_expires_at: None,
            worktree_id: None,
            branch_ref: None,
            anchors: Vec::new(),
            bindings: prism_ir::PlanBinding::default(),
            depends_on: Vec::new(),
            coordination_depends_on: Vec::new(),
            integrated_depends_on: Vec::new(),
            acceptance: Vec::new(),
            validation_refs: Vec::new(),
            is_abstract: false,
            base_revision: WorkspaceRevision::default(),
            priority: None,
            tags: Vec::new(),
            spec_refs: Vec::new(),
            metadata: serde_json::Value::Null,
            git_execution: TaskGitExecution::default(),
        }
    }

    #[test]
    fn wraps_authoritative_payloads_with_schema_version_and_kind() {
        let wrapped =
            wrap_authoritative_payload(&sample_task(), SHARED_COORDINATION_KIND_TASK).unwrap();
        assert_eq!(wrapped["schema_version"], json!(1));
        assert_eq!(wrapped["kind"], json!("coordination_task"));
    }

    #[test]
    fn parses_legacy_raw_payloads_without_envelope() {
        let raw = serde_json::to_vec(&sample_task()).unwrap();
        let parsed = parse_authoritative_payload::<CoordinationTask>(
            &raw,
            "coordination/tasks/sample.json",
            SHARED_COORDINATION_KIND_TASK,
        )
        .unwrap();
        assert_eq!(parsed.title, "Sample");
    }

    #[test]
    fn rejects_newer_schema_versions_with_upgrade_message() {
        let bytes = serde_json::to_vec(&json!({
            "schema_version": 99,
            "kind": "coordination_task",
            "payload": {
                "id": "coord-task:sample",
                "plan": "plan:sample",
                "kind": "Edit",
                "title": "Sample",
            }
        }))
        .unwrap();
        let error = parse_authoritative_payload::<CoordinationTask>(
            &bytes,
            "coordination/tasks/sample.json",
            SHARED_COORDINATION_KIND_TASK,
        )
        .unwrap_err();
        let message = error.to_string();
        assert!(message.contains("schema_version 99"));
        assert!(message.contains("Upgrade PRISM"));
    }
}
