use anyhow::{anyhow, Context, Result};
use prism_coordination::{
    Artifact, ArtifactReview, CoordinationEvent, CoordinationSnapshot, CoordinationSnapshotV2,
    CoordinationTask, Plan, RuntimeDescriptor, WorkClaim,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoordinationStartupCheckpointAuthority {
    pub ref_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoordinationStartupCheckpoint {
    pub version: u32,
    pub materialized_at: u64,
    #[serde(default)]
    pub coordination_revision: u64,
    pub authority: CoordinationStartupCheckpointAuthority,
    pub snapshot: CoordinationSnapshot,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_snapshot_v2: Option<CoordinationSnapshotV2>,
    #[serde(default)]
    pub runtime_descriptors: Vec<RuntimeDescriptor>,
}

impl CoordinationStartupCheckpoint {
    pub const VERSION: u32 = 4;
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct CoordinationStartupCheckpointRevision {
    #[serde(default)]
    pub coordination_revision: u64,
}

pub(crate) fn decode_coordination_startup_checkpoint_compat(
    raw: &str,
) -> Result<CoordinationStartupCheckpoint> {
    let value: Value =
        serde_json::from_str(raw).context("failed to parse startup checkpoint json")?;
    let object = value
        .as_object()
        .ok_or_else(|| anyhow!("coordination startup checkpoint must be a JSON object"))?;
    let version = json_u32(object, &["version"])
        .ok_or_else(|| anyhow!("coordination startup checkpoint is missing `version`"))?;
    let materialized_at = json_u64(object, &["materialized_at", "materializedAt"])
        .ok_or_else(|| anyhow!("coordination startup checkpoint is missing `materialized_at`"))?;
    let authority = serde_json::from_value(
        object
            .get("authority")
            .cloned()
            .ok_or_else(|| anyhow!("coordination startup checkpoint is missing `authority`"))?,
    )
    .context("failed to decode startup checkpoint authority")?;
    let snapshot = decode_coordination_snapshot_compat(
        object
            .get("snapshot")
            .cloned()
            .ok_or_else(|| anyhow!("coordination startup checkpoint is missing `snapshot`"))?,
    )
    .context("failed to decode startup checkpoint snapshot")?;
    let canonical_snapshot_v2 = object
        .get("canonical_snapshot_v2")
        .or_else(|| object.get("canonicalSnapshotV2"))
        .cloned()
        .filter(|value| !value.is_null())
        .map(serde_json::from_value)
        .transpose()
        .context("failed to decode startup checkpoint canonical snapshot v2")?;
    let runtime_descriptors = object
        .get("runtime_descriptors")
        .or_else(|| object.get("runtimeDescriptors"))
        .cloned()
        .filter(|value| !value.is_null())
        .map(serde_json::from_value)
        .transpose()
        .context("failed to decode startup checkpoint runtime descriptors")?
        .unwrap_or_default();
    Ok(CoordinationStartupCheckpoint {
        version,
        materialized_at,
        coordination_revision: json_u64(object, &["coordination_revision", "coordinationRevision"])
            .unwrap_or_default(),
        authority,
        snapshot,
        canonical_snapshot_v2,
        runtime_descriptors,
    })
}

pub(crate) fn decode_coordination_startup_checkpoint_revision_compat(
    raw: &str,
) -> Result<CoordinationStartupCheckpointRevision> {
    let value: Value =
        serde_json::from_str(raw).context("failed to parse startup checkpoint json")?;
    let object = value
        .as_object()
        .ok_or_else(|| anyhow!("coordination startup checkpoint must be a JSON object"))?;
    Ok(CoordinationStartupCheckpointRevision {
        coordination_revision: json_u64(object, &["coordination_revision", "coordinationRevision"])
            .unwrap_or_default(),
    })
}

fn decode_coordination_snapshot_compat(value: Value) -> Result<CoordinationSnapshot> {
    let object = value
        .as_object()
        .ok_or_else(|| anyhow!("coordination snapshot must be a JSON object"))?;
    Ok(CoordinationSnapshot {
        plans: decode_snapshot_records::<Plan, _>(object, "plans", decode_plan_compat)?,
        tasks: decode_snapshot_records::<CoordinationTask, _>(
            object,
            "tasks",
            serde_json::from_value,
        )?,
        claims: decode_snapshot_records::<WorkClaim, _>(object, "claims", serde_json::from_value)?,
        artifacts: decode_snapshot_records::<Artifact, _>(
            object,
            "artifacts",
            serde_json::from_value,
        )?,
        reviews: decode_snapshot_records::<ArtifactReview, _>(
            object,
            "reviews",
            serde_json::from_value,
        )?,
        events: decode_snapshot_records::<CoordinationEvent, _>(
            object,
            "events",
            serde_json::from_value,
        )?,
        next_plan: json_u64(object, &["next_plan", "nextPlan"]).unwrap_or_default(),
        next_task: json_u64(object, &["next_task", "nextTask"]).unwrap_or_default(),
        next_claim: json_u64(object, &["next_claim", "nextClaim"]).unwrap_or_default(),
        next_artifact: json_u64(object, &["next_artifact", "nextArtifact"]).unwrap_or_default(),
        next_review: json_u64(object, &["next_review", "nextReview"]).unwrap_or_default(),
    })
}

fn decode_snapshot_records<T, F>(
    object: &serde_json::Map<String, Value>,
    key: &str,
    mut decode: F,
) -> Result<Vec<T>>
where
    F: FnMut(Value) -> Result<T, serde_json::Error>,
{
    let Some(value) = object.get(key) else {
        return Ok(Vec::new());
    };
    let array = value
        .as_array()
        .ok_or_else(|| anyhow!("coordination snapshot `{key}` must be an array"))?;
    array
        .iter()
        .cloned()
        .map(|entry| decode(entry).with_context(|| format!("failed to decode `{key}` entry")))
        .collect()
}

fn decode_plan_compat(value: Value) -> Result<Plan, serde_json::Error> {
    serde_json::from_value::<Plan>(value.clone()).or_else(|primary_error| {
        let payload = value
            .get("payload")
            .cloned()
            .unwrap_or_else(|| value.clone());
        let plan_value = payload.get("plan").cloned().unwrap_or(payload);
        serde_json::from_value(plan_value).map_err(|_| primary_error)
    })
}

fn json_u64(object: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|key| object.get(*key).and_then(Value::as_u64))
}

fn json_u32(object: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<u32> {
    json_u64(object, keys).and_then(|value| u32::try_from(value).ok())
}
