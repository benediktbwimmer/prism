use prism_curator::{CandidateMemory, CuratorProposal};
use prism_ir::TaskId;
use serde_json::{json, Map, Value};

fn ensure_object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        Value::Null => Map::new(),
        other => {
            let mut map = Map::new();
            map.insert("value".to_string(), other);
            map
        }
    }
}

pub(crate) fn manual_memory_metadata(existing: Value, task_id: &TaskId) -> Value {
    let mut metadata = ensure_object(existing);
    metadata.insert("task_id".to_string(), Value::String(task_id.0.to_string()));
    metadata.insert(
        "provenance".to_string(),
        json!({
            "origin": "manual_store",
            "kind": "manual_memory",
        }),
    );
    metadata.entry("evidence".to_string()).or_insert_with(|| {
        json!({
            "eventIds": [],
            "validationChecks": [],
            "coChangeLineages": [],
        })
    });
    Value::Object(metadata)
}

pub(crate) fn curator_memory_metadata(
    proposal: &CuratorProposal,
    candidate: &CandidateMemory,
    task_id: &TaskId,
    job_id: &str,
    proposal_index: usize,
    extra: Value,
) -> Value {
    let mut metadata = ensure_object(extra);
    let proposal_kind = match proposal {
        CuratorProposal::StructuralMemory(_) => "structural_memory",
        CuratorProposal::SemanticMemory(_) => "semantic_memory",
        CuratorProposal::RiskSummary(_) => "risk_summary",
        CuratorProposal::ValidationRecipe(_) => "validation_recipe",
        CuratorProposal::InferredEdge(_) => "inferred_edge",
    };

    metadata.insert("task_id".to_string(), Value::String(task_id.0.to_string()));
    metadata.insert(
        "curator".to_string(),
        json!({
            "jobId": job_id,
            "proposalIndex": proposal_index,
            "kind": proposal_kind,
            "category": candidate.category.clone(),
        }),
    );
    metadata.insert(
        "provenance".to_string(),
        json!({
            "origin": "curator",
            "kind": proposal_kind,
            "category": candidate.category.clone(),
            "jobId": job_id,
            "proposalIndex": proposal_index,
        }),
    );
    metadata.insert(
        "evidence".to_string(),
        json!({
            "eventIds": candidate
                .evidence
                .event_ids
                .iter()
                .map(|id| id.0.clone())
                .collect::<Vec<_>>(),
            "validationChecks": candidate.evidence.validation_checks.clone(),
            "coChangeLineages": candidate
                .evidence
                .co_change_lineages
                .iter()
                .map(|id| id.0.clone())
                .collect::<Vec<_>>(),
        }),
    );
    metadata.insert(
        "rationale".to_string(),
        Value::String(candidate.rationale.clone()),
    );
    if let Some(category) = &candidate.category {
        metadata.insert("category".to_string(), Value::String(category.clone()));
    }
    if matches!(
        proposal,
        CuratorProposal::StructuralMemory(_) | CuratorProposal::ValidationRecipe(_)
    ) {
        let signal_count = candidate.evidence.event_ids.len()
            + candidate.evidence.validation_checks.len()
            + candidate.evidence.co_change_lineages.len();
        metadata.insert(
            "structuralRule".to_string(),
            json!({
                "kind": candidate
                    .category
                    .clone()
                    .unwrap_or_else(|| "structural_rule".to_string()),
                "promoted": true,
                "signalCount": signal_count,
            }),
        );
    }
    Value::Object(metadata)
}

pub(crate) fn task_journal_memory_metadata(
    existing: Value,
    task_id: &TaskId,
    disposition: &str,
) -> Value {
    let mut metadata = ensure_object(existing);
    metadata.insert("task_id".to_string(), Value::String(task_id.0.to_string()));
    metadata.insert(
        "provenance".to_string(),
        json!({
            "origin": "task_journal",
            "kind": "task_summary",
            "disposition": disposition,
        }),
    );
    metadata.insert(
        "taskLifecycle".to_string(),
        json!({
            "disposition": disposition,
            "closed": true,
        }),
    );
    metadata.entry("evidence".to_string()).or_insert_with(|| {
        json!({
            "eventIds": [],
            "validationChecks": [],
            "coChangeLineages": [],
        })
    });
    Value::Object(metadata)
}
